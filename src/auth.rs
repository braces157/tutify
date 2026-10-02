use crate::service::{FailureKind, Provider, ServiceFailure};
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

pub(crate) mod doctor;
mod identity;
use identity::{AccountIdentity, AccountRelation, bind_streaming, require_same, token_identity};

#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    // Compatibility alias from pre-migration builds, never a stable Web API ID.
    #[serde(default)]
    pub account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<AccountIdentity>,
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

fn credential_tokens(
    result: std::result::Result<String, keyring::Error>,
) -> Result<Option<Tokens>> {
    match result {
        Ok(value) => {
            let tokens: Tokens = serde_json::from_str(&value).map_err(|_| {
                anyhow::anyhow!(
                    "Saved login is damaged; preserve credentials and local files before recovery"
                )
            })?;
            if let Some(identity) = &tokens.identity {
                identity.validate()?;
            }
            Ok(
                (!tokens.access_token.is_empty() && !tokens.refresh_token.is_empty())
                    .then_some(tokens),
            )
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error)
            .context("Cannot read Windows Credential Manager; saved logins have not been changed"),
    }
}

fn known_account_id(id: &str) -> Option<&str> {
    let id = id.trim();
    (!id.is_empty() && !id.eq_ignore_ascii_case("unknown")).then_some(id)
}

fn saved_login_state(catalog: Option<&Tokens>, streaming: Option<&Tokens>) -> Result<(bool, bool)> {
    let catalog_saved = catalog.is_some();
    let streaming_saved = match catalog.zip(streaming) {
        Some((catalog, streaming)) => {
            let catalog = token_identity(catalog, false)?;
            let streaming = token_identity(streaming, true)?;
            if known_account_id(&catalog.stable_web_api_id).is_some()
                && known_account_id(&streaming.stable_web_api_id).is_some()
            {
                catalog.relation(&streaming) == AccountRelation::Same
                    && known_account_id(&catalog.streaming_username).is_some()
                    && catalog.streaming_username == streaming.streaming_username
            } else {
                // Backward-compatible local check for credentials created by
                // the previous verifier. Never compare a stable ID to a username.
                known_account_id(&catalog.legacy_web_api_id).is_some()
                    && catalog.legacy_web_api_id == streaming.streaming_username
            }
        }
        None => streaming.is_some(),
    };
    Ok((catalog_saved, streaming_saved))
}

/// Reauthentication is not a reset operation. Unknown or different identities
/// leave every local file intact; a deliberate switch uses backup/logout.
fn check_account_state(
    store: &Storage,
    previous: Option<&AccountIdentity>,
    verified: &AccountIdentity,
) -> Result<()> {
    if let Some(previous) = previous {
        require_same(previous.relation(verified))?;
    } else if ["queue.json", "cache.json", "stats.json"]
        .iter()
        .any(|name| store.root.join(name).exists())
    {
        require_same(AccountRelation::Ambiguous)?;
    }
    Ok(())
}

async fn prepare_catalog_identity(
    store: &Storage,
    previous: Option<&TokenManager>,
    mut verified: AccountIdentity,
    profile_endpoint: &str,
) -> Result<AccountIdentity> {
    let previous = if let Some(manager) = previous {
        let tokens = manager.state.lock().await.clone();
        let previous = token_identity(&tokens, false)?;
        if previous.relation(&verified) == AccountRelation::Same
            || (known_account_id(&previous.stable_web_api_id).is_some()
                && known_account_id(&verified.stable_web_api_id).is_some())
        {
            Some(previous)
        } else {
            Some(profile_identity_at(&manager.access().await?, profile_endpoint).await.context(
                "Previous account verification failed; credentials and account files are preserved",
            )?)
        }
    } else {
        None
    };
    check_account_state(store, previous.as_ref(), &verified)?;
    if let Some(previous) = previous
        && known_account_id(&previous.stable_web_api_id).is_some()
        && previous.stable_web_api_id == verified.stable_web_api_id
    {
        verified.streaming_username = previous.streaming_username;
    }
    Ok(verified)
}

async fn prepare_streaming_identity<F, Fut>(
    catalog: &TokenManager,
    username: &str,
    profile_endpoint: &str,
    streaming_profile: F,
) -> Result<AccountIdentity>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<AccountIdentity>>,
{
    let access = catalog.access().await?;
    let mut verified = profile_identity_at(&access, profile_endpoint).await?;
    let previous = token_identity(&*catalog.state.lock().await, false)?;
    let stable_comparable = known_account_id(&previous.stable_web_api_id).is_some()
        && known_account_id(&verified.stable_web_api_id).is_some();
    if stable_comparable {
        require_same(previous.relation(&verified))?;
    }
    if stable_comparable
        && known_account_id(&previous.streaming_username).is_some()
        && previous.streaming_username == username
    {
        return bind_streaming(verified, &previous.streaming_username, username);
    }
    // /me.id is the current user's legacy handle, not the immutable account ID.
    // Matching it to the authenticated AP username preserves the old verifier's
    // supported path while recording the stable ID from that same /me response.
    if known_account_id(&verified.legacy_web_api_id) == known_account_id(username)
        && known_account_id(username).is_some()
    {
        return bind_streaming(verified, username, username);
    }
    // A different/absent legacy handle needs independent evidence: /me using
    // the SAME token that authenticated the streaming username. Catalog tokens
    // lack streaming scope, so they must never be used for an AP handshake.
    let streaming = streaming_profile().await.context(
        "Cannot establish a verified catalog/streaming identity mapping; saved account files are preserved",
    )?;
    require_same(verified.relation(&streaming))?;
    if known_account_id(&verified.stable_web_api_id).is_none() {
        verified.stable_web_api_id = streaming.stable_web_api_id;
    }
    bind_streaming(verified, username, username)
}

fn commit_catalog_login(
    store: &Storage,
    config: &Config,
    tokens: &Tokens,
    previous: Option<&Tokens>,
    save: impl Fn(&Tokens) -> Result<()>,
    remove: impl Fn() -> Result<()>,
) -> Result<()> {
    save(tokens)?;
    if let Err(error) = store.save_config(config) {
        let rollback = match previous {
            Some(previous) => save(previous),
            None => remove(),
        };
        rollback.context("Catalog config save failed and credential rollback also failed; local account files are preserved")?;
        return Err(error).context(
            "Catalog config save failed; previous login restored and account files preserved",
        );
    }
    Ok(())
}

fn commit_streaming_login(
    previous_catalog: &Tokens,
    mapped_catalog: &Tokens,
    streaming: &Tokens,
    save: impl Fn(&Tokens, bool) -> Result<()>,
) -> Result<()> {
    save(mapped_catalog, false)?;
    if let Err(error) = save(streaming, true) {
        save(previous_catalog, false).context(
            "Streaming credential save failed and catalog metadata rollback also failed; local account files are preserved",
        )?;
        return Err(error).context("Streaming credential save failed; previous catalog metadata restored and account files preserved");
    }
    Ok(())
}

fn delete_catalog_credential() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
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
    let catalog_tokens = (!config.client_id.is_empty())
        .then(|| credential_tokens(entry()?.get_password()))
        .transpose()?
        .flatten();
    let streaming_tokens = credential_tokens(stream_entry()?.get_password())?;
    let (catalog_saved, streaming_saved) =
        saved_login_state(catalog_tokens.as_ref(), streaming_tokens.as_ref())?;
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
        .user_agent(concat!("Tuitify/", env!("CARGO_PKG_VERSION")))
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
        .map_err(|_| ServiceFailure::spotify(FailureKind::Transport))?;
    let token = decode_token(response).await?;
    println!("Authorization exchanged. Verifying the Spotify account...");
    let mut tokens = Tokens {
        account_id: String::new(),
        identity: None,
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .context("Spotify did not issue a refresh token; retry login")?,
        expires_at: now() + token.expires_in,
    };
    if streaming {
        let username = streaming_username(&tokens.access_token).await?;
        let catalog = TokenManager::load(config)?;
        let mapped = prepare_streaming_identity(
            &catalog,
            &username,
            "https://api.spotify.com/v1/me",
            || profile_identity(&tokens.access_token),
        )
        .await?;
        let previous_catalog = catalog.state.lock().await.clone();
        let mut catalog_tokens = previous_catalog.clone();
        tokens.account_id = username;
        tokens.identity = Some(mapped.clone());
        catalog_tokens.account_id = mapped.legacy_web_api_id.clone();
        catalog_tokens.identity = Some(mapped);
        commit_streaming_login(&previous_catalog, &catalog_tokens, &tokens, save_tokens)?;
    } else {
        let verified = profile_identity(&tokens.access_token).await?;
        let previous_tokens = credential_tokens(entry()?.get_password())?;
        // The old token must use its original client when refreshed. During
        // comparison it is read-only: failed migration cannot replace it.
        let previous_manager = previous_tokens
            .as_ref()
            .map(|tokens| TokenManager::from_tokens(&store.config()?, tokens.clone(), false))
            .transpose()?;
        let verified = prepare_catalog_identity(
            store,
            previous_manager.as_ref(),
            verified,
            "https://api.spotify.com/v1/me",
        )
        .await?;
        tokens.account_id = verified.legacy_web_api_id.clone();
        tokens.identity = Some(verified);
        commit_catalog_login(
            store,
            config,
            &tokens,
            previous_tokens.as_ref(),
            |tokens| save_tokens(tokens, false),
            delete_catalog_credential,
        )?;
        // Keep the established two-login workflow. Invalidating streaming
        // credentials never removes queue/cache/stats, even after a mismatch.
        match stream_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(e) => return Err(e.into()),
        }
    }
    println!("Login saved in Windows Credential Manager.");
    Ok(())
}

async fn decode_token(response: reqwest::Response) -> Result<TokenResponse> {
    match response.status().as_u16() {
        200 => Ok(response
            .json()
            .await
            .map_err(|_| ServiceFailure::spotify(FailureKind::InvalidResponse))?),
        400 | 401 => {
            let mut failure = ServiceFailure::spotify(FailureKind::AuthenticationRequired);
            failure.status = Some(response.status().as_u16());
            Err(failure.into())
        }
        _ => {
            let mut failure = ServiceFailure::from_response(response, Provider::Spotify).await;
            // A missing OAuth endpoint is a service/request failure, not an
            // unavailable song that queue hydration can skip.
            if failure.kind == FailureKind::MissingItem {
                failure.kind = FailureKind::RequestRejected;
            }
            Err(failure.into())
        }
    }
}

async fn profile_identity(token: &str) -> Result<AccountIdentity> {
    profile_identity_at(token, "https://api.spotify.com/v1/me").await
}

/// Librespot's welcome packet returns the authenticated canonical username.
/// The handshake uses no streaming-client Web API quota. Only ambiguous identity
/// mappings need a separate profile request in prepare_streaming_identity.
async fn streaming_username(token: &str) -> Result<String> {
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
    if known_account_id(&id).is_none() {
        bail!("Spotify streaming did not return an account ID; retry setup");
    }
    Ok(id.trim().to_owned())
}

async fn profile_identity_at(token: &str, endpoint: &str) -> Result<AccountIdentity> {
    let client = http_client()?;
    for attempt in 0..2 {
        let response = client
            .get(endpoint)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| ServiceFailure::spotify(FailureKind::Transport))?;
        match response.status().as_u16() {
            200 => (),
            _ => {
                let failure = ServiceFailure::from_response(response, Provider::Spotify).await;
                let wait = failure
                    .retry_at
                    .map(|until| until.saturating_duration_since(Instant::now()));
                if attempt == 0
                    && failure.kind == FailureKind::RateLimited
                    && wait.is_some_and(|wait| wait <= Duration::from_secs(30))
                {
                    let wait = wait.unwrap();
                    println!(
                        "Spotify rate limit (HTTP 429). Waiting {} seconds before one verification retry; Premium is not the issue.",
                        wait.as_secs()
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                return Err(failure.into());
            }
        }
        let profile: serde_json::Value = response
            .json()
            .await
            .map_err(|_| ServiceFailure::spotify(FailureKind::InvalidResponse))?;
        return AccountIdentity::from_profile(&profile)
            .map_err(|_| ServiceFailure::spotify(FailureKind::InvalidResponse).into());
    }
    unreachable!()
}

#[cfg(test)]
async fn profile_id_at(token: &str, endpoint: &str) -> Result<String> {
    let identity = profile_identity_at(token, endpoint).await?;
    Ok(known_account_id(&identity.legacy_web_api_id)
        .unwrap_or(&identity.stable_web_api_id)
        .to_owned())
}

pub fn retry_delay(header: Option<&str>) -> Duration {
    crate::service::retry_after(header).unwrap_or(Duration::from_secs(60))
}

#[derive(Clone)]
pub struct TokenManager {
    state: Arc<Mutex<Tokens>>,
    client: reqwest::Client,
    client_id: String,
    endpoint: String,
    persist: bool,
    streaming: bool,
    read_only: bool,
    cooldown: Arc<Mutex<Option<ServiceFailure>>>,
}

impl TokenManager {
    pub(crate) fn offline() -> Result<Self> {
        Ok(Self {
            state: Arc::new(Mutex::new(Tokens {
                access_token: "offline-demo".into(),
                refresh_token: String::new(),
                expires_at: u64::MAX,
                account_id: "demo".into(),
                identity: None,
            })),
            client: http_client()?,
            client_id: "demo".into(),
            endpoint: "http://127.0.0.1:1/offline".into(),
            persist: false,
            streaming: false,
            read_only: false,
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    pub fn load(config: &Config) -> Result<Self> {
        let text = entry()?
            .get_password()
            .context("No saved Spotify login; run tuitify auth first")?;
        let tokens = credential_tokens(Ok(text))?.context("Saved login has no reusable tokens")?;
        Self::from_tokens(config, tokens, true)
    }
    fn from_tokens(config: &Config, tokens: Tokens, persist: bool) -> Result<Self> {
        token_identity(&tokens, false)?;
        Ok(Self {
            state: Arc::new(Mutex::new(tokens)),
            client: http_client()?,
            client_id: config.client_id.clone(),
            endpoint: TOKEN_URL.into(),
            persist,
            streaming: false,
            read_only: false,
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    pub fn load_streaming() -> Result<Self> {
        let text = stream_entry()?
            .get_password()
            .context("No streaming login; run tuitify auth --streaming")?;
        let tokens =
            credential_tokens(Ok(text))?.context("Streaming login has no reusable tokens")?;
        token_identity(&tokens, true)?;
        Ok(Self {
            state: Arc::new(Mutex::new(tokens)),
            client: http_client()?,
            client_id: STREAMING_CLIENT_ID.to_owned(),
            endpoint: TOKEN_URL.into(),
            persist: true,
            streaming: true,
            read_only: false,
            cooldown: Arc::new(Mutex::new(None)),
        })
    }
    pub async fn access(&self) -> Result<String> {
        self.token(None).await
    }
    pub(crate) async fn verify_streaming_username(&self, username: &str) -> Result<()> {
        let identity = token_identity(&*self.state.lock().await, true)?;
        bind_streaming(identity.clone(), &identity.streaming_username, username)
            .map(|_| ())
            .context("Streaming connection account could not be verified; run tuitify auth --streaming --force")
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
        // Doctor must never rotate credentials, even remotely. Its optional
        // GET probes require an already usable token and never refresh a 401.
        if self.read_only {
            return Err(ServiceFailure::spotify(FailureKind::AuthenticationRequired).into());
        }
        if let Some(failure) = *self.cooldown.lock().await
            && failure.active()
        {
            return Err(failure.into());
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
            .map_err(|_| ServiceFailure::spotify(FailureKind::Transport))?;
        let token = match decode_token(response).await {
            Ok(token) => token,
            Err(error) => {
                if let Some(failure) = error.downcast_ref::<ServiceFailure>() {
                    if failure.throttled() {
                        *self.cooldown.lock().await = Some(*failure);
                    }
                    if failure.kind == FailureKind::AuthenticationRequired && self.streaming {
                        return Err(error).context(
                            "Streaming login revoked; run tuitify auth --streaming --force",
                        );
                    }
                }
                return Err(error);
            }
        };
        let updated = Tokens {
            account_id: state.account_id.clone(),
            identity: state.identity.clone(),
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
    pub(crate) fn mock_for_account(endpoint: String, client_id: &str, account_id: &str) -> Self {
        let mut manager = Self::mock(endpoint, false);
        manager.client_id = client_id.into();
        manager.state.try_lock().unwrap().account_id = account_id.into();
        manager
    }

    #[cfg(test)]
    pub fn mock(endpoint: String, expired: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(Tokens {
                access_token: "old".into(),
                refresh_token: "refresh".into(),
                expires_at: if expired { 0 } else { now() + 3600 },
                account_id: String::new(),
                identity: None,
            })),
            client: http_client().unwrap(),
            client_id: "test-client".into(),
            endpoint,
            persist: false,
            streaming: false,
            read_only: false,
            cooldown: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(test)]
mod tests;
