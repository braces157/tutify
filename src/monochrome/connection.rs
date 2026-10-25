//! Keep FFmpeg on the address family that supplied the verified audio header.
use anyhow::{Context, Result, ensure};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};

pub(super) struct Ipv6Dns;

impl reqwest::dns::Resolve for Ipv6Dns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(SocketAddr::is_ipv6)
                .collect();
            if addresses.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "No IPv6 address",
                )
                .into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A configured HTTP proxy's address must never be mistaken for the audio host.
pub(super) async fn verified_peer(response: &reqwest::Response) -> Option<SocketAddr> {
    if response.url().scheme() != "https" {
        return None;
    }
    let peer = response.remote_addr().filter(SocketAddr::is_ipv6)?;
    let host = response.url().host_str()?;
    let port = response.url().port_or_known_default()?;
    let mut addresses = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .ok()?
    .ok()?;
    addresses.find(|address| *address == peer)
}

/// A decoder-scoped CONNECT tunnel. TLS, SNI and HTTP ranges remain with FFmpeg.
/// The listener can reach only the verified endpoint, never an arbitrary host.
pub(crate) struct Tunnel {
    pub(crate) url: String,
    task: JoinHandle<()>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Tunnel {
    pub(crate) async fn for_audio(audio: &crate::volta::Audio) -> Result<Option<Self>> {
        let Some(address) = audio.connect_addr else {
            return Ok(None);
        };
        let url = url::Url::parse(&audio.url)?;
        ensure!(url.scheme() == "https", "Unsupported tunneled audio URL");
        let authority = format!(
            "{}:{}",
            url.host_str().context("Audio host missing")?,
            url.port_or_known_default().context("Audio port missing")?
        );
        Ok(Some(Self::bind(address, authority).await?))
    }

    async fn bind(address: SocketAddr, authority: String) -> Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let url = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        if connections.len() < 4 {
                            let authority = authority.clone();
                            connections.spawn(async move { let _ = relay(stream, address, &authority).await; });
                        }
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Ok(Self { url, task })
    }
}

async fn relay(mut client: TcpStream, address: SocketAddr, authority: &str) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            ensure!(header.len() < 8192, "Audio tunnel request too large");
            header.push(client.read_u8().await?);
        }
        let first = std::str::from_utf8(&header)?
            .lines()
            .next()
            .unwrap_or_default();
        ensure!(
            first == format!("CONNECT {authority} HTTP/1.1")
                || first == format!("CONNECT {authority} HTTP/1.0"),
            "Unsupported audio tunnel destination"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("Audio tunnel handshake timed out")??;
    let mut upstream =
        tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(address)).await??;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tunnel_forwards_bytes_and_drop_closes_connections() -> Result<()> {
        let upstream = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let tunnel = Tunnel::bind(upstream.local_addr()?, "tracks.example:443".into()).await?;
        let mut client = TcpStream::connect(tunnel.url.trim_start_matches("http://")).await?;
        client
            .write_all(b"CONNECT tracks.example:443 HTTP/1.1\r\nHost: tracks.example:443\r\n\r\n")
            .await?;
        let (mut server, _) =
            tokio::time::timeout(Duration::from_secs(2), upstream.accept()).await??;
        let mut reply = [0; 39];
        client.read_exact(&mut reply).await?;
        assert_eq!(&reply, b"HTTP/1.1 200 Connection Established\r\n\r\n");
        client.write_all(b"encrypted request").await?;
        let mut request = [0; 17];
        server.read_exact(&mut request).await?;
        assert_eq!(&request, b"encrypted request");
        server.write_all(b"encrypted response").await?;
        let mut response = [0; 18];
        client.read_exact(&mut response).await?;
        assert_eq!(&response, b"encrypted response");
        drop(tunnel);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), client.read(&mut [0])).await??,
            0
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server.read(&mut [0])).await??,
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn tunnel_rejects_other_destinations_without_connecting() -> Result<()> {
        let upstream = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let tunnel = Tunnel::bind(upstream.local_addr()?, "tracks.example:443".into()).await?;
        let mut client = TcpStream::connect(tunnel.url.trim_start_matches("http://")).await?;
        client
            .write_all(b"CONNECT other.example:443 HTTP/1.1\r\n\r\n")
            .await?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), client.read(&mut [0])).await??,
            0
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(30), upstream.accept())
                .await
                .is_err()
        );
        Ok(())
    }
}
