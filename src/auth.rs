use crate::storage::{Config, Storage};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};

pub const REDIRECT: &str = "http://127.0.0.1:8989/callback";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SCOPES: &str =
    "user-read-private user-library-read playlist-read-private playlist-read-collaborative";

#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    #[serde(default)]
    pub account_id: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("Tuitify", "spotify-oauth")?)
}
fn stream_entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new("Tuitify", "spotify-streaming-oauth")?)
}
fn save_tokens(tokens: &Tokens, streaming: bool) -> Result<()> {
    (if streaming { stream_entry()? } else { entry()? })
        .set_password(&serde_json::to_string(tokens)?)
        .context("Cannot save tokens in Windows Credential Manager")
}
pub fn delete_tokens() -> Result<()> {
    for entry in [entry()?, stream_entry()?] {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Tuitify/0.1.0")
        .build()?)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Strict callback validation; no token/code/error description is ever included in diagnostics.
pub fn validate_callback(target: &str, expected_state: &str) -> Result<String> {
    validate_callback_path(target, expected_state, "/callback")
}
fn validate_callback_path(target: &str, expected_state: &str, path: &str) -> Result<String> {
    let url = url::Url::parse(&format!("http://127.0.0.1:8989{target}"))?;
    if url.path() != path {
        bail!("Unexpected callback path");
    }
    let params: Vec<_> = url.query_pairs().collect();
    let values = |key: &str| {
        params
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.to_string())
            .collect::<Vec<_>>()
    };
    let state = values("state");
    if state.len() != 1 || state[0] != expected_state {
        bail!("OAuth state validation failed; run tuitify auth again");
    }
    if !values("error").is_empty() {
        bail!("Spotify authorization was declined; run tuitify auth again when ready");
    }
    let code = values("code");
    if code.len() != 1 || code[0].is_empty() {
        bail!("Missing or duplicate authorization code");
    }
    Ok(code[0].clone())
}

pub async fn login(store: &Storage, client_id: Option<String>, streaming: bool) -> Result<()> {
    let mut config = store.config()?;
    if let Some(id) = client_id {
        config.client_id = id.trim().to_owned();
    }
    if config.client_id.is_empty() {
        println!(
            "Create a Spotify Developer app and register {REDIRECT}\nPaste its client ID (never a client secret):"
        );
        let mut id = String::new();
        std::io::stdin().read_line(&mut id)?;
        config.client_id = id.trim().into();
    }
    if config.client_id.len() != 32 || !config.client_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Client ID must be 32 hexadecimal characters");
    }
    let oauth_id = if streaming {
        librespot_core::config::SessionConfig::default().client_id
    } else {
        config.client_id.clone()
    };
    let redirect = if streaming {
        "http://127.0.0.1:8989/login"
    } else {
        REDIRECT
    };
    let callback_path = if streaming { "/login" } else { "/callback" };
    let scopes = if streaming {
        "streaming user-read-private"
    } else {
        SCOPES
    };
    let listener = TcpListener::bind("127.0.0.1:8989")
        .await
        .context("Port 8989 is busy; close the other login process and retry")?;
    let verifier = random_secret();
    let state = random_secret();
    let mut url = url::Url::parse("https://accounts.spotify.com/authorize")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", oauth_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect),
        ("scope", scopes),
        ("state", &state),
        ("code_challenge_method", "S256"),
        ("code_challenge", &pkce_challenge(&verifier)),
    ]);
    println!(
        "Complete Spotify login in your browser. Waiting up to five minutes.\nIf the browser did not open, visit:\n{url}"
    );
    let _ = webbrowser::open(url.as_str());
    let code = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0u8; 8192]; let mut used = 0;
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if used == request.len() { bail!("Callback too large"); }
                    let n = stream.read(&mut request[used..]).await?; if n == 0 { bail!("Incomplete callback"); }
                    used += n; if request[..used].windows(4).any(|w| w == b"\r\n\r\n") { break; }
                }
                Ok::<_, anyhow::Error>(())
            }).await;
            if !matches!(read, Ok(Ok(()))) { continue; }
            let request = String::from_utf8_lossy(&request[..used]);
            let mut parts = request.lines().next().unwrap_or_default().split_whitespace();
            let method = parts.next().unwrap_or_default(); let target = parts.next().unwrap_or_default();
            if method != "GET" || !target.starts_with(&format!("{callback_path}?")) {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await; continue;
            }
            let result = if streaming { validate_callback_path(target, &state, callback_path) } else { validate_callback(target, &state) };
            let (status, body) = if result.is_ok() { ("200 OK", "Login received. Return to Tuitify to see whether setup succeeded.") } else { ("400 Bad Request", "Login validation failed. Return to Tuitify and run auth again.") };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(response.as_bytes()).await;
            return result;
        }
    }).await.context("Login timed out; run tuitify auth again")??;
    let response = http_client()?
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", oauth_id.as_str()),
            ("redirect_uri", redirect),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .context("Cannot reach Spotify login; check your network and run auth again")?;
    let token = decode_token(response).await?;
    let tokens = Tokens {
        account_id: profile_id(&token.access_token).await?,
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .context("Spotify did not issue a refresh token; retry login")?,
        expires_at: now() + token.expires_in,
    };
    if streaming {
        let catalog = TokenManager::load(&config)?;
        let id = profile_id(&catalog.access().await?).await?;
        if id != tokens.account_id {
            bail!(
                "Streaming and catalog logins belong to different Spotify accounts. Run tuitify auth --streaming and choose the same account."
            );
        }
    }
    // Every successful explicit login starts a clean account queue; no cross-account reuse.
    store.clear_queue()?;
    store.save_config(&config)?;
    if !streaming {
        match stream_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(e) => return Err(e.into()),
        }
    }
    save_tokens(&tokens, streaming)?;
    println!(
        "Login saved in Windows Credential Manager. {}",
        if streaming {
            "Run tuitify to open the player."
        } else {
            "Next run tuitify auth --streaming to authorize standalone audio."
        }
    );
    Ok(())
}

async fn decode_token(response: reqwest::Response) -> Result<TokenResponse> {
    match response.status().as_u16() {
        200 => Ok(response
            .json()
            .await
            .context("Invalid Spotify token response")?),
        400 | 401 => bail!("Spotify login expired or was revoked; run tuitify auth"),
        429 => bail!(
            "Spotify login quota reached; wait {} seconds before retrying",
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("60")
        ),
        status => {
            bail!("Spotify login returned HTTP {status}; check your connection and retry later")
        }
    }
}

async fn profile_id(token: &str) -> Result<String> {
    let response = http_client()?
        .get("https://api.spotify.com/v1/me")
        .bearer_auth(token)
        .send()
        .await
        .context("Cannot verify Spotify account; check network and retry login")?;
    if !response.status().is_success() {
        bail!(
            "Cannot verify Spotify account (HTTP {}). Check Premium and app access, then retry login.",
            response.status().as_u16()
        );
    }
    let profile: serde_json::Value = response.json().await?;
    Ok(profile["id"]
        .as_str()
        .context("Spotify account ID missing")?
        .to_owned())
}

pub fn retry_delay(header: Option<&str>) -> Duration {
    header
        .and_then(|s| {
            s.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                httpdate::parse_http_date(s)
                    .ok()
                    .and_then(|date| date.duration_since(SystemTime::now()).ok())
            })
        })
        .unwrap_or(Duration::from_secs(60))
}

#[derive(Clone)]
pub struct TokenManager {
    state: Arc<Mutex<Tokens>>,
    client: reqwest::Client,
    client_id: String,
    endpoint: String,
    persist: bool,
    streaming: bool,
    cooldown: Arc<Mutex<Option<Instant>>>,
}

impl TokenManager {
    pub fn load(config: &Config) -> Result<Self> {
        let text = entry()?
            .get_password()
            .context("No saved Spotify login; run tuitify auth first")?;
        Ok(Self {
            state: Arc::new(Mutex::new(
                serde_json::from_str(&text).context("Saved login is damaged; run tuitify auth")?,
            )),
            client: http_client()?,
            client_id: config.client_id.clone(),
            endpoint: TOKEN_URL.into(),
            persist: true,
            streaming: false,
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    pub fn load_streaming() -> Result<Self> {
        let text = stream_entry()?
            .get_password()
            .context("No streaming login; run tuitify auth --streaming")?;
        Ok(Self {
            state: Arc::new(Mutex::new(
                serde_json::from_str(&text)
                    .context("Streaming login damaged; run tuitify auth --streaming")?,
            )),
            client: http_client()?,
            client_id: librespot_core::config::SessionConfig::default().client_id,
            endpoint: TOKEN_URL.into(),
            persist: true,
            streaming: true,
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    pub async fn access(&self) -> Result<String> {
        self.token(None).await
    }
    pub async fn refresh_rejected(&self, rejected: &str) -> Result<String> {
        self.token(Some(rejected)).await
    }
    async fn token(&self, rejected: Option<&str>) -> Result<String> {
        let mut state = self.state.lock().await;
        let rejected_current = rejected.is_some_and(|t| t == state.access_token);
        if !rejected_current && state.expires_at > now() + 60 {
            return Ok(state.access_token.clone());
        }
        if let Some(until) = *self.cooldown.lock().await {
            if until > Instant::now() {
                bail!(
                    "Spotify login rate limit; wait {} seconds before retrying",
                    until.saturating_duration_since(Instant::now()).as_secs() + 1
                );
            }
        }
        let response = self
            .client
            .post(&self.endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", self.client_id.as_str()),
                ("refresh_token", state.refresh_token.as_str()),
            ])
            .send()
            .await
            .context("Token refresh failed; check your connection and retry")?;
        if response.status().as_u16() == 429 {
            let delay = retry_delay(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
            );
            *self.cooldown.lock().await = Instant::now().checked_add(delay);
        }
        let token = decode_token(response).await.with_context(|| {
            if self.streaming {
                "Streaming login failed; use tuitify auth --streaming if revoked"
            } else {
                "Catalog login failed; run tuitify auth if revoked"
            }
        })?;
        let updated = Tokens {
            account_id: state.account_id.clone(),
            access_token: token.access_token,
            refresh_token: token
                .refresh_token
                .unwrap_or_else(|| state.refresh_token.clone()),
            expires_at: now() + token.expires_in,
        };
        if self.persist {
            save_tokens(&updated, self.streaming)?;
        }
        *state = updated;
        Ok(state.access_token.clone())
    }
    #[cfg(test)]
    pub fn mock(endpoint: String, expired: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(Tokens {
                access_token: "old".into(),
                refresh_token: "refresh".into(),
                expires_at: if expired { 0 } else { now() + 3600 },
                account_id: String::new(),
            })),
            client: http_client().unwrap(),
            client_id: "test-client".into(),
            endpoint,
            persist: false,
            streaming: false,
            cooldown: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };
    #[test]
    fn oauth_validation() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(
            validate_callback("/callback?state=abc&code=xyz", "abc").unwrap(),
            "xyz"
        );
        for target in [
            "/callback?code=x",
            "/callback?state=wrong&code=x",
            "/callback?state=abc&state=abc&code=x",
            "/callback?state=abc&error=access_denied",
            "/other?state=abc&code=x",
            "/callback?state=abc&code=x&code=y",
        ] {
            assert!(validate_callback(target, "abc").is_err());
        }
    }
    #[tokio::test]
    async fn refresh_is_serialized_and_preserves_refresh_token() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("refresh_token=refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token":"new", "expires_in":3600})),
            )
            .expect(1)
            .mount(&s)
            .await;
        let manager = TokenManager::mock(format!("{}/token", s.uri()), true);
        let (a, b) = tokio::join!(manager.access(), manager.access());
        assert_eq!(a.unwrap(), "new");
        assert_eq!(b.unwrap(), "new");
        assert_eq!(manager.state.lock().await.refresh_token, "refresh");
    }
    #[tokio::test]
    async fn revoked_refresh_is_actionable() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&s)
            .await;
        assert!(
            TokenManager::mock(s.uri(), true)
                .access()
                .await
                .unwrap_err()
                .to_string()
                .contains("tuitify auth")
        );
    }
    #[tokio::test]
    async fn token_quota_respects_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&server)
            .await;
        let manager = TokenManager::mock(server.uri(), true);
        assert!(manager.access().await.is_err());
        assert!(
            manager
                .access()
                .await
                .unwrap_err()
                .to_string()
                .contains("rate limit")
        );
    }
    #[test]
    fn retry_header_seconds_and_http_date() {
        assert_eq!(retry_delay(Some("120")), Duration::from_secs(120));
        let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(90));
        assert!((88..=90).contains(&retry_delay(Some(&date)).as_secs()));
        assert_eq!(retry_delay(Some("invalid")), Duration::from_secs(60));
    }
}
