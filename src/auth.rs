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
/// Shared catalog client used by Spotatui's PKCE flow. It keeps first-run
/// setup usable without asking every user to register a Developer app. Users
/// who need their own quota or app access can override it with `--client-id`.
pub const SHARED_CLIENT_ID: &str = "d420a117a32841c2b3474932e49fb54b";
/// Spotify's desktop/keymaster client is the client ID used by Spotatui and
/// other librespot-based players. Spotify grants this client the streaming
/// product scope that a user-created Web API app may not receive.
pub const STREAMING_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub const STREAMING_REDIRECT: &str = "http://127.0.0.1:8989/login";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const SCOPES: &str =
    "user-read-private user-library-read playlist-read-private playlist-read-collaborative";
const STREAMING_SCOPES: &str = "streaming user-read-playback-state user-modify-playback-state user-read-currently-playing user-library-read user-read-private";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginStep {
    Catalog,
    Streaming,
}

fn setup_steps(
    catalog_saved: bool,
    streaming_saved: bool,
    client_changed: bool,
    force: bool,
    streaming_only: bool,
) -> Vec<LoginStep> {
    let catalog_needed = !catalog_saved || client_changed || (force && !streaming_only);
    let mut steps = Vec::new();
    if catalog_needed {
        steps.push(LoginStep::Catalog);
    }
    // A new catalog login invalidates the previous account's streaming login.
    if catalog_needed || !streaming_saved || force {
        steps.push(LoginStep::Streaming);
    }
    steps
}

fn credential_saved(result: std::result::Result<String, keyring::Error>) -> Result<bool> {
    match result {
        Ok(value) => Ok(serde_json::from_str::<Tokens>(&value).is_ok_and(|tokens| {
            !tokens.access_token.is_empty() && !tokens.refresh_token.is_empty()
        })),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(error)
            .context("Cannot read Windows Credential Manager; saved logins have not been changed"),
    }
}

/// Check locally first: expired access tokens still have reusable refresh tokens.
/// Re-running setup never contacts Spotify when both credentials are present.
pub async fn setup(
    store: &Storage,
    client_id: Option<String>,
    force: bool,
    streaming_only: bool,
) -> Result<()> {
    let config = store.config()?;
    let requested_id = client_id.map(|id| id.trim().to_owned());
    let client_changed = requested_id
        .as_ref()
        .is_some_and(|id| id != &config.client_id);
    let catalog_saved = !config.client_id.is_empty() && credential_saved(entry()?.get_password())?;
    let streaming_saved = credential_saved(stream_entry()?.get_password())?;
    let steps = setup_steps(
        catalog_saved,
        streaming_saved,
        client_changed,
        force,
        streaming_only,
    );
    if steps.is_empty() {
        return Ok(());
    }
    println!(
        "Tuitify setup: {} browser login step(s) remaining. Use the same Spotify account for both logins.",
        steps.len()
    );
    for (index, step) in steps.iter().enumerate() {
        let streaming = *step == LoginStep::Streaming;
        println!(
            "\nLogin {} of {}: {}",
            index + 1,
            steps.len(),
            if streaming {
                "standalone audio (Spotify for Desktop)"
            } else {
                "search, playlists, and liked songs"
            }
        );
        login(
            store,
            if streaming {
                None
            } else {
                requested_id.clone()
            },
            streaming,
        )
        .await?;
    }
    Ok(())
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

fn oauth_settings(client_id: &str, streaming: bool) -> (&str, &str, &str, &str) {
    if streaming {
        (
            STREAMING_CLIENT_ID,
            STREAMING_REDIRECT,
            "/login",
            STREAMING_SCOPES,
        )
    } else if client_id == SHARED_CLIENT_ID {
        (SHARED_CLIENT_ID, STREAMING_REDIRECT, "/login", SCOPES)
    } else {
        (client_id, REDIRECT, "/callback", SCOPES)
    }
}

async fn bind_callback_listener() -> Result<TcpListener> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpListener::bind("127.0.0.1:8989").await {
            Ok(listener) => return Ok(listener),
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < deadline =>
            {
                // The previous browser callback can keep the port in use for
                // a short moment while Windows closes the connection.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => {
                return Err(error)
                    .context("Port 8989 is busy; close the other login process and retry");
            }
        }
    }
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
            "Using the shared Spotify catalog client. To use your own app instead, rerun with --client-id YOUR_CLIENT_ID.\n"
        );
        config.client_id = SHARED_CLIENT_ID.to_owned();
    }
    if config.client_id.len() != 32 || !config.client_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Client ID must be 32 hexadecimal characters");
    }
    // Both Spotatui's shared client and librespot's streaming client have
    // /login registered. A personal catalog app uses /callback, which the
    // user registers in the Developer dashboard.
    let (oauth_id, redirect, callback_path, scopes) = oauth_settings(&config.client_id, streaming);
    // Bind before launching the browser so an immediate redirect cannot race
    // listener startup.
    let listener = bind_callback_listener().await?;
    let verifier = random_secret();
    let state = random_secret();
    let mut url = url::Url::parse("https://accounts.spotify.com/authorize")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", oauth_id),
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
    if let Err(error) = webbrowser::open(url.as_str()) {
        println!(
            "Could not open the browser automatically ({error}); open the URL above manually."
        );
    }
    let (code, mut callback) = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut request = vec![0u8; 8192];
            let mut used = 0;
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if used == request.len() {
                        bail!("Callback too large");
                    }
                    let n = stream.read(&mut request[used..]).await?;
                    if n == 0 {
                        bail!("Incomplete callback");
                    }
                    used += n;
                    if request[..used].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Ok::<_, anyhow::Error>(())
            })
            .await;
            if !matches!(read, Ok(Ok(()))) {
                continue;
            }
            let request = String::from_utf8_lossy(&request[..used]);
            let mut parts = request
                .lines()
                .next()
                .unwrap_or_default()
                .split_whitespace();
            let method = parts.next().unwrap_or_default();
            let target = parts.next().unwrap_or_default();
            if method != "GET" || !target.starts_with(&format!("{callback_path}?")) {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let result = if callback_path == "/callback" {
                validate_callback(target, &state)
            } else {
                validate_callback_path(target, &state, callback_path)
            };
            match result {
                Ok(code) => return Ok((code, stream)),
                Err(error) => {
                    send_callback_result(&mut stream, &Err(anyhow::anyhow!("{error}")), streaming)
                        .await;
                    return Err(error);
                }
            }
        }
    })
    .await
    .context("Login timed out; run tuitify auth again")??;
    println!("Browser authorization received. Exchanging the code with Spotify...");
    let result = finish_login(
        store, &config, streaming, oauth_id, redirect, &code, &verifier,
    )
    .await;
    send_callback_result(&mut callback, &result, streaming).await;
    result
}

fn callback_message(result: &Result<()>, streaming: bool) -> (String, String) {
    match result {
        Ok(()) => (
            "200 OK".into(),
            if streaming {
                "Spotify streaming login saved successfully. Close this tab and return to the terminal to continue.".into()
            } else {
                "Spotify catalog login saved successfully. Close this tab and return to the terminal. Tuitify will automatically open the remaining browser login step.".into()
            },
        ),
        Err(error) => (
            "400 Bad Request".into(),
            format!(
                "Tuitify setup did not finish.\n\n{error:#}\n\nReturn to the terminal. Browser authorization alone does not complete setup."
            ),
        ),
    }
}

async fn send_callback_result(
    stream: &mut tokio::net::TcpStream,
    result: &Result<()>,
    streaming: bool,
) {
    let (status, body) = callback_message(result, streaming);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // A closed browser tab must not prevent terminal login from finishing.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    })
    .await;
}

async fn finish_login(
    store: &Storage,
    config: &Config,
    streaming: bool,
    oauth_id: &str,
    redirect: &str,
    code: &str,
    verifier: &str,
) -> Result<()> {
    let response = http_client()?
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", oauth_id),
            ("redirect_uri", redirect),
            ("code", code),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("Cannot reach Spotify login; check your network and run auth again")?;
    let token = decode_token(response).await?;
    println!("Authorization exchanged. Verifying the Spotify account...");
    let account_id = if streaming {
        streaming_account_id(&token.access_token).await?
    } else {
        profile_id(&token.access_token).await?
    };
    let tokens = Tokens {
        account_id,
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .context("Spotify did not issue a refresh token; retry login")?,
        expires_at: now() + token.expires_in,
    };
    if streaming {
        let catalog = TokenManager::load(config)?;
        let cached_id = catalog.state.lock().await.account_id.clone();
        let id = if cached_id.is_empty() {
            profile_id(&catalog.access().await?).await?
        } else {
            cached_id
        };
        if id != tokens.account_id {
            bail!(
                "Streaming and catalog logins belong to different Spotify accounts. Run tuitify auth --streaming and choose the same account."
            );
        }
    }
    // Every successful explicit login starts a clean account queue; no cross-account reuse.
    store.clear_queue()?;
    store.save_config(config)?;
    if !streaming {
        match stream_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(e) => return Err(e.into()),
        }
    }
    save_tokens(&tokens, streaming)?;
    println!("Login saved in Windows Credential Manager.");
    Ok(())
}

async fn decode_token(response: reqwest::Response) -> Result<TokenResponse> {
    match response.status().as_u16() {
        200 => Ok(response
            .json()
            .await
            .context("Invalid Spotify token response")?),
        400 | 401 => bail!("Spotify login expired or was revoked; run tuitify auth --force"),
        429 => bail!(
            "Spotify login is rate-limited (HTTP 429). Wait at least {} seconds before retrying; this does not indicate a Premium problem.",
            retry_delay(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
            )
            .as_secs()
        ),
        status => {
            bail!("Spotify login returned HTTP {status}; check your connection and retry later")
        }
    }
}

async fn profile_id(token: &str) -> Result<String> {
    profile_id_at(token, "https://api.spotify.com/v1/me").await
}

/// Librespot's welcome packet returns the authenticated canonical username.
/// Streaming authorization must not use the shared streaming client's Web API
/// quota, because Spotify may reject that metadata request for this client.
async fn streaming_account_id(token: &str) -> Result<String> {
    use librespot_core::{authentication::Credentials, config::SessionConfig, session::Session};
    let session = Session::new(
        SessionConfig {
            client_id: STREAMING_CLIENT_ID.to_owned(),
            ..SessionConfig::default()
        },
        None,
    );
    let connection = tokio::time::timeout(
        Duration::from_secs(35),
        session.connect(Credentials::with_access_token(token), false),
    )
    .await;
    // Always tear down the short-lived verification session, including when
    // the connection times out or Spotify rejects the token.
    match connection {
        Ok(Ok(())) => (),
        Ok(Err(_)) => {
            session.shutdown();
            bail!("Spotify streaming authorization failed; check your account and retry setup");
        }
        Err(_) => {
            session.shutdown();
            bail!(
                "Streaming account verification timed out; check your connection and retry setup"
            );
        }
    }
    // `Session::username` is populated by `connect`; reading it before the
    // future completes races the authentication handshake and returns an empty
    // ID on a fresh session.
    let id = session.username();
    session.shutdown();
    if id.is_empty() || id == "UNKNOWN" {
        bail!("Spotify streaming did not return an account ID; retry setup");
    }
    Ok(id)
}

async fn profile_id_at(token: &str, endpoint: &str) -> Result<String> {
    let client = http_client()?;
    for attempt in 0..2 {
        let response = client
            .get(endpoint)
            .bearer_auth(token)
            .send()
            .await
            .context("Cannot verify Spotify account; check network and retry login")?;
        match response.status().as_u16() {
            200 => (),
            429 => {
                let wait = retry_delay(
                    response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()),
                );
                if attempt == 0 && wait <= Duration::from_secs(30) {
                    println!(
                        "Spotify rate limit (HTTP 429). Waiting {} seconds before one verification retry; Premium is not the issue.",
                        wait.as_secs()
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                bail!(
                    "Spotify account verification is rate-limited (HTTP 429). Wait at least {} seconds before retrying login. Premium is not the issue; avoid repeated login attempts during this wait.",
                    wait.as_secs()
                );
            }
            401 => bail!(
                "Spotify account verification rejected the login (HTTP 401); run the same auth command again."
            ),
            403 => bail!(
                "Spotify denied account verification (HTTP 403). Check that this account is allowed in your Developer app and that the app owner has Premium."
            ),
            status => bail!(
                "Spotify account verification returned HTTP {status}. Setup did not finish; retry later."
            ),
        }
        let profile: serde_json::Value = response.json().await?;
        return Ok(profile["id"]
            .as_str()
            .context("Spotify account ID missing")?
            .to_owned());
    }
    unreachable!()
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
            client_id: STREAMING_CLIENT_ID.to_owned(),
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
                "Streaming login failed; use tuitify auth --streaming --force if revoked"
            } else {
                "Catalog login failed; run tuitify auth --force if revoked"
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
    fn setup_only_opens_missing_logins() {
        use LoginStep::*;
        assert_eq!(
            setup_steps(false, false, false, false, false),
            vec![Catalog, Streaming]
        );
        assert_eq!(
            setup_steps(true, false, false, false, false),
            vec![Streaming]
        );
        assert!(setup_steps(true, true, false, false, false).is_empty());
        assert_eq!(
            setup_steps(false, true, false, false, false),
            vec![Catalog, Streaming]
        );
    }
    #[test]
    fn setup_client_change_and_force_invalidate_the_right_credentials() {
        use LoginStep::*;
        assert_eq!(
            setup_steps(true, true, true, false, false),
            vec![Catalog, Streaming]
        );
        assert_eq!(
            setup_steps(true, true, false, true, false),
            vec![Catalog, Streaming]
        );
        assert_eq!(setup_steps(true, true, false, true, true), vec![Streaming]);
        assert_eq!(
            setup_steps(false, true, false, false, true),
            vec![Catalog, Streaming]
        );
        assert!(setup_steps(true, true, false, false, true).is_empty());
    }
    #[test]
    fn saved_refresh_token_skips_browser_even_when_access_token_expired() {
        let tokens = Tokens {
            access_token: "expired".into(),
            refresh_token: "reusable".into(),
            expires_at: 0,
            account_id: "account".into(),
        };
        assert!(credential_saved(Ok(serde_json::to_string(&tokens).unwrap())).unwrap());
        assert!(!credential_saved(Err(keyring::Error::NoEntry)).unwrap());
        assert!(!credential_saved(Ok("broken json".into())).unwrap());
        let empty = Tokens {
            refresh_token: String::new(),
            ..tokens
        };
        assert!(!credential_saved(Ok(serde_json::to_string(&empty).unwrap())).unwrap());
    }
    #[test]
    fn callback_reports_final_success_or_failure() {
        let (status, body) = callback_message(&Ok(()), false);
        assert_eq!(status, "200 OK");
        assert!(body.contains("automatically open the remaining"));
        let (_, body) = callback_message(&Ok(()), true);
        assert!(body.contains("return to the terminal"));
        let (status, body) =
            callback_message(&Err(anyhow::anyhow!("HTTP 429: wait 120 seconds")), false);
        assert_ne!(status, "200 OK");
        assert!(body.contains("setup did not finish"));
        assert!(body.contains("wait 120 seconds"));
        assert!(!body.contains("saved successfully"));
    }

    #[tokio::test]
    async fn verification_long_rate_limit_does_not_retry_or_blame_premium() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "120"))
            .expect(1)
            .mount(&server)
            .await;
        let message = profile_id_at("test-token", &server.uri())
            .await
            .unwrap_err()
            .to_string();
        assert!(message.contains("120 seconds"));
        assert!(message.contains("Premium is not the issue"));
    }

    #[tokio::test]
    async fn verification_short_rate_limit_retries_once_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"account"})),
            )
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            profile_id_at("test-token", &server.uri()).await.unwrap(),
            "account"
        );
    }

    #[tokio::test]
    async fn verification_repeated_rate_limit_is_bounded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .expect(2)
            .mount(&server)
            .await;
        assert!(profile_id_at("test-token", &server.uri()).await.is_err());
    }

    #[tokio::test]
    async fn verification_denied_access_does_not_retry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        assert!(
            profile_id_at("test-token", &server.uri())
                .await
                .unwrap_err()
                .to_string()
                .contains("allowed in your Developer app")
        );
    }
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
        assert_eq!(
            validate_callback_path("/login?state=abc&code=xyz", "abc", "/login").unwrap(),
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
    #[test]
    fn oauth_settings_match_spotatui_redirects_and_scopes() {
        let (_, redirect, path, scopes) = oauth_settings(SHARED_CLIENT_ID, false);
        assert_eq!(redirect, STREAMING_REDIRECT);
        assert_eq!(path, "/login");
        assert!(scopes.contains("playlist-read-private"));

        let (_, redirect, path, scopes) = oauth_settings("0123456789abcdef0123456789abcdef", false);
        assert_eq!(redirect, REDIRECT);
        assert_eq!(path, "/callback");
        assert_eq!(scopes, SCOPES);

        let (client, redirect, path, scopes) = oauth_settings("ignored", true);
        assert_eq!(client, STREAMING_CLIENT_ID);
        assert_eq!(redirect, STREAMING_REDIRECT);
        assert_eq!(path, "/login");
        assert!(scopes.contains("streaming"));
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
