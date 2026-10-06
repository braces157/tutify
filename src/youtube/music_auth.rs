//! Google sign-in is confined to a new, dedicated browser profile.
//! Only music.youtube.com session cookies are read; credentials use user-scoped DPAPI.
use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::PCWSTR,
};

const MAGIC: &[u8] = b"TUITIFY_YTM_AUTH_1\0";
const MAX_AUTH: usize = 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    version: u32,
    pub headers: HashMap<String, String>,
}

impl Credentials {
    pub fn from_headers(value: &Value) -> Result<Self> {
        let mut headers = HashMap::new();
        for (name, value) in value
            .as_object()
            .context("YouTube Music session headers are missing")?
        {
            let name = name.to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "cookie" | "authorization" | "x-goog-authuser" | "x-goog-pageid" | "user-agent"
            ) {
                let value = value
                    .as_str()
                    .context("Invalid YouTube Music session header")?;
                ensure!(
                    !value.contains(['\r', '\n', '\0']) && value.len() <= 32 * 1024,
                    "Invalid YouTube Music session header"
                );
                if name == "x-goog-pageid" && value.is_empty() {
                    continue;
                }
                headers.insert(name, value.to_owned());
            }
        }
        headers.insert("x-origin".into(), "https://music.youtube.com".into());
        headers.insert("accept".into(), "*/*".into());
        headers.insert("content-type".into(), "application/json".into());
        headers
            .entry("x-goog-authuser".into())
            .or_insert_with(|| "0".into());
        ensure!(
            headers
                .get("authorization")
                .is_some_and(|value| value.contains("SAPISIDHASH"))
                && headers.get("cookie").is_some_and(|value| value
                    .split(';')
                    .any(|cookie| cookie.trim().starts_with("__Secure-3PAPISID=")
                        && cookie.trim().len() > "__Secure-3PAPISID=".len())),
            "Sign in to YouTube Music and open Library before connecting"
        );
        let credentials = Self {
            version: 1,
            headers,
        };
        ensure!(
            serde_json::to_vec(&credentials)?.len() <= MAX_AUTH,
            "YouTube Music session headers exceed the size limit"
        );
        Ok(credentials)
    }
}

pub fn credential_path() -> Result<PathBuf> {
    Ok(crate::storage::Storage::local_read_only()?
        .root
        .join("youtube/music-auth.dpapi"))
}

fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_AUTH + 4096,
        "Invalid encrypted YouTube Music credentials"
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI allocates the output; copy it before freeing with LocalFree.
    unsafe {
        let status = if protect {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        status.context("Windows could not protect/read this YouTube Music connection; run 'tuitify youtube login' to reconnect")?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(bytes)
    }
}

pub fn save(path: &Path, credentials: &Credentials) -> Result<()> {
    let bytes = serde_json::to_vec(credentials)?;
    let encrypted = crypt(&bytes, true)?;
    let parent = path.parent().context("Connection path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    file.write_all(MAGIC)?;
    file.write_all(&encrypted)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| error.error)
        .context("Could not save the encrypted YouTube Music connection")?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Option<Credentials>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_AUTH + 4096 + MAGIC.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_AUTH + 4096 + MAGIC.len(),
        "YouTube Music connection file is too large; preserved for recovery"
    );
    let encrypted = bytes.strip_prefix(MAGIC).context("Invalid YouTube Music connection file; run 'tuitify youtube login'. The file was preserved")?;
    let plaintext = crypt(encrypted, false)?;
    let credentials: Credentials = serde_json::from_slice(&plaintext)
        .context("Invalid YouTube Music connection; run 'tuitify youtube login'")?;
    ensure!(
        credentials.version == 1,
        "Unsupported YouTube Music connection version; preserved for recovery"
    );
    Credentials::from_headers(&serde_json::to_value(&credentials.headers)?).map(Some)
}

pub fn logout() -> Result<()> {
    match std::fs::remove_file(credential_path()?) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    println!(
        "YouTube Music disconnected. Queue, playlists cached in memory and Spotify credentials were not changed. Restart the player to apply."
    );
    Ok(())
}

fn browser_path() -> Result<PathBuf> {
    // New profile only. Never inspect/copy an existing browser's cookie store.
    for relative in [
        "Google/Chrome/Application/chrome.exe",
        "Microsoft/Edge/Application/msedge.exe",
    ] {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(variable) {
                let path = PathBuf::from(&root).join(relative);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    bail!("Google sign-in needs Google Chrome or Microsoft Edge installed")
}

fn music_page(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("music.youtube.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
    })
}

fn cookie_headers(cookies: &Value, account: &Value) -> Result<Credentials> {
    let records = cookies
        .as_array()
        .context("YouTube Music session cookies unavailable")?;
    let mut pairs = Vec::new();
    for cookie in records {
        let domain = cookie["domain"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('.');
        if !matches!(domain, "youtube.com" | "music.youtube.com") {
            continue;
        }
        let name = cookie["name"].as_str().context("Invalid session cookie")?;
        let value = cookie["value"].as_str().context("Invalid session cookie")?;
        ensure!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                && !value.contains([';', '\r', '\n', '\0'])
                && value.len() < 16384,
            "Invalid YouTube Music session cookie"
        );
        pairs.push(format!("{name}={value}"));
    }
    // ytmusicapi regenerates the SAPISID hash from this cookie on every request.
    Credentials::from_headers(&json!({
        "cookie":pairs.join("; "), "authorization":"SAPISIDHASH generated-by-ytmusicapi",
        "x-goog-authuser":account["authUser"].as_str().unwrap_or("0"),
        "x-goog-pageid":account["brand"].as_str().unwrap_or(""),
        "user-agent":account["userAgent"].as_str().unwrap_or("Mozilla/5.0")
    }))
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn cdp(socket: &mut Socket, id: &mut u64, method: &str, params: Value) -> Result<Value> {
    *id += 1;
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({"id":*id,"method":method,"params":params})
                .to_string()
                .into(),
        ))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "The dedicated sign-in window disconnected; retry 'tuitify youtube login'"
            )
        })?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(message) = socket.next().await {
            if let tokio_tungstenite::tungstenite::Message::Text(text) = message.map_err(|_| {
                anyhow::anyhow!(
                    "The dedicated sign-in window disconnected; retry 'tuitify youtube login'"
                )
            })? {
                ensure!(
                    text.len() <= 1024 * 1024,
                    "Sign-in response exceeded the size limit"
                );
                let value: Value =
                    serde_json::from_str(&text).context("Invalid sign-in window response")?;
                if value["id"].as_u64() == Some(*id) {
                    ensure!(
                        value.get("error").is_none(),
                        "Could not read the dedicated sign-in window"
                    );
                    return Ok(value["result"].clone());
                }
            }
        }
        bail!("The dedicated sign-in window closed")
    })
    .await
    .context("The dedicated sign-in window did not respond")?
}

pub async fn login() -> Result<()> {
    let client = super::music::Client::discover()?
        .context("Run 'tuitify youtube music-setup' before connecting your account")?;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let profile = tempfile::Builder::new()
        .prefix("tuitify-music-login-")
        .tempdir()?;
    let mut browser = std::process::Command::new(browser_path()?)
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .arg(format!("--remote-debugging-port={port}"))
        .args([
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--app=https://music.youtube.com",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not open the dedicated Google sign-in window")?;
    println!("Sign in with Google in the dedicated YouTube Music window, then click Library.");
    println!(
        "Tuitify reads this window's YouTube Music session only and saves it encrypted for your Windows user. Your Google password stays in the browser. Ctrl-C cancels."
    );
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let session = async {
        let target = loop {
            if let Ok(response) = http
                .get(format!("http://127.0.0.1:{port}/json/list"))
                .send()
                .await
                && let Ok(targets) = response.json::<Vec<Value>>().await
                && let Some(target) = targets.into_iter().find(|target| {
                    target["type"].as_str() == Some("page")
                        && target["url"].as_str().is_some_and(music_page)
                })
            {
                break target;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        };
        let websocket = target["webSocketDebuggerUrl"]
            .as_str()
            .context("Sign-in window has no connection endpoint")?;
        let endpoint = url::Url::parse(websocket).context("Invalid sign-in connection endpoint")?;
        ensure!(
            endpoint.scheme() == "ws"
                && matches!(endpoint.host_str(), Some("127.0.0.1" | "localhost"))
                && endpoint.port() == Some(port),
            "Unexpected sign-in connection endpoint"
        );
        let (mut socket, _) = tokio_tungstenite::connect_async(websocket)
            .await
            .context("Could not connect to the dedicated sign-in window")?;
        let mut id = 0;
        loop {
            let frame = cdp(&mut socket, &mut id, "Page.getFrameTree", json!({})).await?;
            // No network interception or page inspection while Google sign-in is open.
            if frame["frameTree"]["frame"]["url"]
                .as_str()
                .is_some_and(music_page)
            {
                let details = cdp(&mut socket, &mut id, "Runtime.evaluate", json!({"expression":"({signedIn: globalThis.ytcfg?.get?.('LOGGED_IN') === true, ready: location.pathname.startsWith('/library'), authUser: String(globalThis.ytcfg?.get?.('SESSION_INDEX') ?? '0'), brand: String(globalThis.ytcfg?.get?.('DELEGATED_SESSION_ID') ?? ''), userAgent: navigator.userAgent})", "returnByValue":true})).await?;
                let account = &details["result"]["value"];
                if account["signedIn"].as_bool() == Some(true)
                    && account["ready"].as_bool() == Some(true)
                {
                    let cookies = cdp(
                        &mut socket,
                        &mut id,
                        "Network.getCookies",
                        json!({"urls":["https://music.youtube.com/"]}),
                    )
                    .await?;
                    let credentials = cookie_headers(&cookies["cookies"], account)?;
                    client.validate_credentials(&credentials).await?;
                    save(&credential_path()?, &credentials)?;
                    // Close only the single window created for this sign-in.
                    let _ = cdp(&mut socket, &mut id, "Page.close", json!({})).await;
                    println!(
                        "YouTube Music connected. Run 'tuitify youtube'; 2 opens playlists, 3 opens Liked Songs, and s cycles shuffle modes."
                    );
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
    };
    let result = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(600), session) => result.unwrap_or_else(|_| Err(anyhow::anyhow!("Google sign-in timed out; no connection was saved. Close the dedicated window and retry 'tuitify youtube login'."))),
        _ = tokio::signal::ctrl_c() => Err(anyhow::anyhow!("Google sign-in cancelled; no connection was saved. You can close the dedicated window.")),
    };
    // TempDir only contains the profile created above. Chrome may release files
    // shortly after closing its app window. Never terminate other browser/music processes.
    let mut window_open = true;
    for _ in 0..20 {
        if http
            .get(format!("http://127.0.0.1:{port}/json/version"))
            .send()
            .await
            .is_err()
        {
            window_open = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = browser.try_wait();
    if window_open {
        let _ = profile.keep(); // Do not remove files while its window is still open.
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn headers() -> Value {
        json!({"Cookie":"SAPISID=private-cookie; __Secure-3PAPISID=private-cookie", "Authorization":"SAPISIDHASH secret", "X-Goog-AuthUser":"1"})
    }

    #[tokio::test]
    async fn browser_rpc_ignores_events_and_redacts_protocol_errors() {
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let request = socket.next().await.unwrap().unwrap().into_text().unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            assert_eq!(request["method"], "Page.getFrameTree");
            for response in [
                json!({"method":"Page.frameNavigated","params":{}}),
                json!({"id":999,"result":{"ignored":true}}),
                json!({"id":request["id"],"result":{"frameTree":{"frame":{"url":"https://accounts.google.com/signin"}}}}),
            ] {
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
            let request = socket.next().await.unwrap().unwrap().into_text().unwrap();
            let request: Value = serde_json::from_str(&request).unwrap();
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"error":{"message":"private-session-token"}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        });
        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
            .await
            .unwrap();
        let mut id = 0;
        let frame = cdp(&mut socket, &mut id, "Page.getFrameTree", json!({}))
            .await
            .unwrap();
        assert!(!music_page(
            frame["frameTree"]["frame"]["url"].as_str().unwrap()
        ));
        let error = cdp(&mut socket, &mut id, "Page.getFrameTree", json!({}))
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not read the dedicated sign-in window"
        );
        assert!(!format!("{error:#}").contains("private-session-token"));
        server.await.unwrap();
    }

    #[test]
    fn encrypted_credentials_roundtrip_without_plaintext_or_spotify_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("music-auth.dpapi");
        let credentials = Credentials::from_headers(&headers()).unwrap();
        save(&path, &credentials).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(14).any(|bytes| bytes == b"private-cookie"));
        assert_eq!(
            load(&path).unwrap().unwrap().headers["x-goog-authuser"],
            "1"
        );
        save(&path, &credentials).unwrap();
        std::fs::write(&path, "corrupt").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "corrupt");
    }

    #[test]
    fn browser_connection_is_scoped_to_music_and_does_not_inspect_google_signin() {
        assert!(music_page("https://music.youtube.com/library"));
        assert!(!music_page("https://accounts.google.com/signin"));
        assert!(!music_page("https://music.youtube.com.evil.test/library"));
        let cookies = json!([{"name":"__Secure-3PAPISID","value":"music-token","domain":".youtube.com"}, {"name":"SID","value":"unrelated-secret","domain":".google.com"}]);
        let credentials = cookie_headers(&cookies, &json!({"authUser":"2"})).unwrap();
        assert!(credentials.headers["cookie"].contains("music-token"));
        assert!(!credentials.headers["cookie"].contains("unrelated-secret"));
        assert_eq!(credentials.headers["x-goog-authuser"], "2");
        assert!(
            Credentials::from_headers(
                &json!({"cookie":"SAPISID=abc\r\nInjected: yes", "authorization":"SAPISIDHASH x"})
            )
            .is_err()
        );
    }
}
