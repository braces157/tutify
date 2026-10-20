//! Personal Volta applications use the documented client-credentials API.
//! Never copy browser tokens or ship a shared client secret.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub(crate) mod catalog;
pub(crate) mod matching;
pub(crate) mod playback;
#[cfg(test)]
mod tests;

const BASE: &str = "https://api.volta-music.com/api/v1";
const AGENT: &str = concat!(
    "Tuitify/",
    env!("CARGO_PKG_VERSION"),
    " (personal Volta client)"
);

#[derive(Serialize, Deserialize)]
pub(crate) struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}
impl Credentials {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.client_id.starts_with("volta_id_") && self.client_secret.starts_with("volta_sk_"),
            "Supply the client ID and secret from a personal Volta application"
        );
        ensure!(
            self.client_id.len() <= 256
                && self.client_secret.len() <= 1024
                && !self.client_id.chars().any(char::is_whitespace)
                && !self.client_secret.chars().any(char::is_whitespace),
            "Invalid Volta credentials"
        );
        Ok(())
    }
}
fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("Tuitify", "volta-application")?)
}
fn credentials() -> Result<Credentials> {
    if let (Ok(client_id), Ok(client_secret)) = (
        std::env::var("TUITIFY_VOLTA_CLIENT_ID"),
        std::env::var("TUITIFY_VOLTA_CLIENT_SECRET"),
    ) {
        return Ok(Credentials {
            client_id,
            client_secret,
        });
    }
    let secret = entry()?.get_password().context("Volta is not connected. Create an application with catalog:read and stream:read at https://account.volta-music.com/apps, then run 'tuitify volta login'")?;
    serde_json::from_str(&secret)
        .context("Saved Volta connection is invalid; reconnect with 'tuitify volta login'")
}
pub(crate) fn available() -> bool {
    credentials().is_ok_and(|credentials| credentials.validate().is_ok())
        && playback::ffmpeg().is_ok()
}
pub(crate) async fn login(from_stdin: bool) -> Result<()> {
    let credentials = if from_stdin {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::io::stdin().take(4097).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4096, "Volta credentials input is too large");
        serde_json::from_slice(&bytes)
            .context("Expected JSON containing client_id and client_secret")?
    } else {
        println!(
            "Create a personal application at https://account.volta-music.com/apps with catalog:read and stream:read."
        );
        println!("Optional library browsing uses library:read and playlists:read.");
        print!("Client ID: ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut client_id = String::new();
        std::io::stdin().read_line(&mut client_id)?;
        print!("Client secret (hidden): ");
        std::io::stdout().flush()?;
        let client_secret = read_secret()?;
        Credentials {
            client_id: client_id.trim().into(),
            client_secret,
        }
    };
    let client = Client::new(credentials, BASE)?;
    client.token().await?;
    entry()?.set_password(&serde_json::to_string(&*client.credentials)?)?;
    println!(
        "Volta connected. Credentials are stored in Windows Credential Manager. Run 'tuitify' for automatic lossless playback."
    );
    Ok(())
}
fn read_secret() -> Result<String> {
    use crossterm::{
        event::{self, Event, KeyCode, KeyModifiers},
        terminal,
    };
    terminal::enable_raw_mode()?;
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    let _raw = Raw;
    let mut secret = String::new();
    loop {
        if let Event::Key(key) = event::read()? {
            if !matches!(
                key.kind,
                event::KeyEventKind::Press | event::KeyEventKind::Repeat
            ) {
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    println!("\r");
                    return Ok(secret);
                }
                KeyCode::Esc => bail!("Volta connection cancelled"),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    bail!("Volta connection cancelled")
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char(c) if !c.is_control() && secret.len() < 1024 => secret.push(c),
                _ => (),
            }
        }
    }
}
pub(crate) fn logout() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => (),
        Err(error) => return Err(error.into()),
    }
    println!(
        "Saved Volta credentials removed. Revoke the application at https://account.volta-music.com/apps to invalidate its keys. Your queue is preserved."
    );
    Ok(())
}

pub(crate) fn key(id: &str) -> Option<&str> {
    id.strip_prefix("volta:").filter(|id| valid_key(id))
}
fn valid_key(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}
pub(crate) fn track_id(input: &str) -> Option<String> {
    let input = input.trim();
    let id = key(input).or_else(|| valid_key(input).then_some(input))?;
    Some(format!("volta:{id}"))
}

struct Token {
    value: String,
    expires: tokio::time::Instant,
}
#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    base: String,
    credentials: Arc<Credentials>,
    token: Arc<Mutex<Option<Token>>>,
    read_gate: Arc<Mutex<tokio::time::Instant>>,
    stream_gate: Arc<Mutex<tokio::time::Instant>>,
    token_gate: Arc<Mutex<tokio::time::Instant>>,
    cooldown: Arc<Mutex<Option<crate::service::ServiceFailure>>>,
}
impl Client {
    #[cfg(test)]
    pub(crate) fn mock(base: &str) -> Self {
        Self::new(
            Credentials {
                client_id: "volta_id_test".into(),
                client_secret: "volta_sk_test".into(),
            },
            base,
        )
        .unwrap()
    }
    pub(crate) fn load() -> Result<Self> {
        Self::new(credentials()?, BASE)
    }
    fn new(credentials: Credentials, base: &str) -> Result<Self> {
        credentials.validate()?;
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(AGENT)
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base: base.trim_end_matches('/').into(),
            credentials: Arc::new(credentials),
            token: Arc::new(Mutex::new(None)),
            read_gate: Arc::new(Mutex::new(tokio::time::Instant::now())),
            stream_gate: Arc::new(Mutex::new(tokio::time::Instant::now())),
            token_gate: Arc::new(Mutex::new(tokio::time::Instant::now())),
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    fn failure(kind: crate::service::FailureKind) -> anyhow::Error {
        crate::service::ServiceFailure::new(crate::service::Provider::Volta, kind).into()
    }
    async fn check_cooldown(&self) -> Result<()> {
        if let Some(failure) = *self.cooldown.lock().await
            && failure.active()
        {
            return Err(failure.into());
        }
        Ok(())
    }
    async fn checked(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        if response.status().is_success() {
            return Ok(response);
        }
        let failure = crate::service::ServiceFailure::from_response(
            response,
            crate::service::Provider::Volta,
        )
        .await;
        if failure.throttled() {
            *self.cooldown.lock().await = Some(failure);
        }
        Err(failure.into())
    }
    async fn json(response: reqwest::Response) -> Result<Value> {
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Self::failure(crate::service::FailureKind::Transport))?
        {
            ensure!(
                body.len() + chunk.len() <= 4 * 1024 * 1024,
                "Volta response exceeded the metadata limit"
            );
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map_err(|_| Self::failure(crate::service::FailureKind::InvalidResponse))
    }
    async fn token(&self) -> Result<String> {
        self.check_cooldown().await?;
        let mut token = self.token.lock().await;
        if let Some(token) = token.as_ref()
            && token.expires > tokio::time::Instant::now()
        {
            return Ok(token.value.clone());
        }
        let mut issuance = self.token_gate.lock().await;
        tokio::time::sleep_until(*issuance).await;
        *issuance = tokio::time::Instant::now() + Duration::from_millis(6100);
        let response = self
            .http
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.credentials.client_id),
                ("client_secret", &self.credentials.client_secret),
            ])
            .send()
            .await
            .map_err(|_| Self::failure(crate::service::FailureKind::Transport))?;
        let value = Self::json(self.checked(response).await?).await?;
        let access = value["access_token"]
            .as_str()
            .filter(|s| s.starts_with("volta_at_") && s.len() < 4096)
            .context("Volta returned an invalid access token")?;
        let scopes = value["scope"].as_str().unwrap_or("");
        ensure!(
            ["catalog:read", "stream:read"]
                .iter()
                .all(|scope| scopes.split_whitespace().any(|s| s == *scope)),
            "Volta application needs catalog:read and stream:read permissions"
        );
        let ttl = value["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .min(3600)
            .saturating_sub(30)
            .max(1);
        *token = Some(Token {
            value: access.into(),
            expires: tokio::time::Instant::now() + Duration::from_secs(ttl),
        });
        Ok(access.into())
    }
    async fn wait(&self, stream: bool) -> Result<()> {
        self.check_cooldown().await?;
        let mut gate = if stream {
            self.stream_gate.lock().await
        } else {
            self.read_gate.lock().await
        };
        tokio::time::sleep_until(*gate).await;
        *gate =
            tokio::time::Instant::now() + Duration::from_millis(if stream { 6100 } else { 1100 });
        self.check_cooldown().await
    }
    async fn response(&self, path: &str, query: &[(&str, String)]) -> Result<reqwest::Response> {
        self.wait(false).await?;
        if path == "/stream" {
            self.wait(true).await?;
        }
        let token = self.token().await?;
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .query(query)
            .send()
            .await
            .map_err(|_| Self::failure(crate::service::FailureKind::Transport))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            *self.token.lock().await = None;
        }
        self.checked(response).await
    }
    pub(crate) async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        Self::json(self.response(path, query).await?).await
    }
    pub(crate) async fn stream(&self, id: &str) -> Result<Audio> {
        let id = key(id).context("Choose a Volta track from Volta search")?;
        let response = self
            .response(
                "/stream",
                &[
                    ("track_id", id.into()),
                    ("quality", "lossless_hq".into()),
                    ("intent", "active".into()),
                    ("codecs", "flac,aac,mp3".into()),
                ],
            )
            .await?;
        let cookies = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|h| h.to_str().ok())
            .filter_map(|h| h.split(';').next())
            .filter(|cookie| !cookie.contains(['\r', '\n']) && cookie.contains('='))
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .join("; ");
        let value = Self::json(response).await?;
        let raw = value["stream_url"]
            .as_str()
            .context("Volta returned no playback link")?;
        let base = url::Url::parse(&self.base)?;
        let url = base
            .join(raw)
            .map_err(|_| Self::failure(crate::service::FailureKind::InvalidResponse))?;
        ensure!(
            url.origin() == base.origin() && url.username().is_empty() && url.password().is_none(),
            "Volta returned an unsupported audio relay (origin {})",
            url.origin().ascii_serialization()
        );
        let quality = crate::audio_quality::AudioQuality::volta(&value);
        let mut headers = format!("User-Agent: {AGENT}\r\n");
        if !cookies.is_empty() {
            headers.push_str(&format!("Cookie: {cookies}\r\n"));
        }
        Ok(Audio {
            url: url.into(),
            headers,
            quality,
            obtained: std::time::Instant::now(),
            source: "Volta",
        })
    }
}
#[derive(Clone)]
pub(crate) struct Audio {
    pub(crate) url: String,
    pub(crate) headers: String,
    pub(crate) quality: crate::audio_quality::AudioQuality,
    pub(crate) obtained: std::time::Instant,
    pub(crate) source: &'static str,
}
impl Audio {
    fn fresh(&self) -> bool {
        self.obtained.elapsed()
            < Duration::from_secs(if self.source == "YouTube" { 300 } else { 25 })
            && url::Url::parse(&self.url).ok().is_some_and(|url| {
                url.query_pairs()
                    .find(|(k, _)| k == "exp" || k == "expire")
                    .and_then(|(_, v)| v.parse::<u64>().ok())
                    .is_none_or(|expires| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .is_ok_and(|now| now.as_secs().saturating_add(15) < expires)
                    })
            })
    }
}
