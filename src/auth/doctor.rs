//! Credential inspection shares production parsing, but never saves, refreshes,
//! authenticates, or opens an access-point streaming session.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Missing,
    Present,
    Expired,
    Invalid,
    Unavailable,
}

pub(crate) struct Credentials {
    pub catalog: State,
    pub streaming: State,
    pub same_account: Option<bool>,
    catalog_tokens: Option<Tokens>,
}

fn inspect(result: std::result::Result<String, keyring::Error>) -> (State, Option<Tokens>) {
    if result.as_ref().is_ok_and(|text| text.len() > 256 * 1024) {
        return (State::Invalid, None);
    }
    let unreadable = result
        .as_ref()
        .is_err_and(|error| !matches!(error, keyring::Error::NoEntry));
    match credential_tokens(result) {
        Ok(Some(tokens)) => (
            if tokens.expires_at > now().saturating_add(60) {
                State::Present
            } else {
                State::Expired
            },
            Some(tokens),
        ),
        Ok(None) => (State::Missing, None),
        Err(_) => (
            if unreadable {
                State::Unavailable
            } else {
                State::Invalid
            },
            None,
        ),
    }
}

impl Credentials {
    pub fn read() -> Self {
        let read = |streaming| {
            (if streaming { stream_entry() } else { entry() })
                .map_err(|_| {
                    keyring::Error::NoStorageAccess(Box::new(std::io::Error::other(
                        "Credential store unavailable",
                    )))
                })
                .and_then(|entry| entry.get_password())
        };
        Self::from_results(read(false), read(true))
    }

    fn from_results(
        catalog: std::result::Result<String, keyring::Error>,
        streaming: std::result::Result<String, keyring::Error>,
    ) -> Self {
        let (catalog, catalog_tokens) = inspect(catalog);
        let (streaming, streaming_tokens) = inspect(streaming);
        let same_account =
            catalog_tokens
                .as_ref()
                .zip(streaming_tokens.as_ref())
                .map(|(catalog, streaming)| {
                    saved_login_state(Some(catalog), Some(streaming))
                        .is_ok_and(|(_, streaming_ok)| streaming_ok)
                });
        Self {
            catalog,
            streaming,
            same_account,
            catalog_tokens,
        }
    }

    pub fn catalog_manager(&self, config: &Config) -> Result<TokenManager> {
        if self.catalog != State::Present {
            bail!("Doctor requires an unexpired saved catalog token; run tuitify auth, then retry");
        }
        let mut manager = TokenManager::from_tokens(
            config,
            self.catalog_tokens
                .clone()
                .context("No saved catalog token")?,
            false,
        )?;
        manager.read_only = true;
        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn saved(expired: bool, account: &str) -> String {
        serde_json::to_string(&Tokens {
            access_token: "sensitive-access-token".into(),
            refresh_token: "sensitive-refresh-token".into(),
            expires_at: if expired { 0 } else { now() + 3600 },
            account_id: account.into(),
            identity: None,
        })
        .unwrap()
    }

    #[test]
    fn missing_damaged_expired_and_mismatched_credentials_are_distinct() {
        let credentials = Credentials::from_results(
            Err(keyring::Error::NoEntry),
            Ok("secret damaged JSON".into()),
        );
        assert_eq!(credentials.catalog, State::Missing);
        assert_eq!(credentials.streaming, State::Invalid);
        assert_eq!(credentials.same_account, None);
        let credentials =
            Credentials::from_results(Ok(saved(true, "one")), Ok(saved(false, "two")));
        assert_eq!(credentials.catalog, State::Expired);
        assert_eq!(credentials.streaming, State::Present);
        assert_eq!(credentials.same_account, Some(false));
        assert!(credentials.catalog_manager(&Config::default()).is_err());
        let credentials =
            Credentials::from_results(Ok(saved(false, "one")), Ok(saved(false, "one")));
        assert_eq!(credentials.same_account, Some(true));
    }

    #[test]
    fn unavailable_vault_and_oversized_credentials_are_safe() {
        let credentials = Credentials::from_results(
            Err(keyring::Error::NoStorageAccess(Box::new(
                std::io::Error::other("private user path"),
            ))),
            Ok("s".repeat(256 * 1024 + 1)),
        );
        assert_eq!(credentials.catalog, State::Unavailable);
        assert_eq!(credentials.streaming, State::Invalid);
    }

    #[tokio::test]
    async fn doctor_401_and_expiry_never_refresh_or_persist_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        let credentials =
            Credentials::from_results(Ok(saved(false, "one")), Err(keyring::Error::NoEntry));
        let mut manager = credentials.catalog_manager(&Config::default()).unwrap();
        manager.endpoint = format!("{}/token", server.uri());
        assert!(!manager.persist);
        assert!(manager.read_only);
        let catalog = crate::catalog::Catalog::mock_with_tokens(&server.uri(), manager.clone());
        // Catalog's actual GET path would ordinarily refresh a rejected token.
        let error = catalog.get("/me", &[]).await.unwrap_err();
        assert!(ServiceFailure::is(
            &error,
            FailureKind::AuthenticationRequired
        ));
        manager.state.lock().await.expires_at = 0;
        assert!(manager.access().await.is_err());
        assert_eq!(
            manager.state.lock().await.access_token,
            "sensitive-access-token"
        );
        assert_eq!(
            manager.state.lock().await.refresh_token,
            "sensitive-refresh-token"
        );
        server.verify().await;
    }
}
