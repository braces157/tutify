//! A failed or incomplete profile read is never evidence of a free account.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum AccountPlan {
    Premium,
    Free,
    /// Development-mode profiles can legitimately omit subscription details.
    Unavailable,
}

impl AccountPlan {
    fn from_profile(profile: &serde_json::Value) -> Result<Self> {
        if profile
            .get("product")
            .is_none_or(serde_json::Value::is_null)
        {
            return Ok(Self::Unavailable);
        }
        match profile["product"].as_str() {
            Some("premium") => Ok(Self::Premium),
            Some("free" | "open") => Ok(Self::Free),
            _ => Err(ServiceFailure::spotify(FailureKind::InvalidResponse).into()),
        }
    }
}

impl TokenManager {
    pub(crate) async fn account_key(&self) -> Result<String> {
        let saved = token_identity(&*self.state.lock().await, false)?;
        let identity = serde_json::to_vec(&saved)?;
        Ok(format!("{:x}", Sha256::digest(identity)))
    }

    /// Subscription fields can be absent on the Web API. A previously connected
    /// streaming account can still report its product without opening audio.
    pub(crate) async fn streaming_plan(&self) -> Result<Option<AccountPlan>> {
        use librespot_core::{
            authentication::Credentials, config::SessionConfig, session::Session,
        };
        let Some(streaming) = TokenManager::load_optional_streaming()? else {
            return Ok(None);
        };
        let catalog_tokens = self.state.lock().await.clone();
        let streaming_tokens = streaming.state.lock().await.clone();
        if !saved_login_state(Some(&catalog_tokens), Some(&streaming_tokens))?.1 {
            bail!("Saved Spotify accounts do not match; credentials and queues were preserved");
        }
        struct Probe(Session);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.shutdown();
            }
        }
        let probe = Probe(Session::new(
            SessionConfig {
                client_id: STREAMING_CLIENT_ID.into(),
                ..SessionConfig::default()
            },
            None,
        ));
        let token = streaming.access().await?;
        tokio::time::timeout(
            Duration::from_secs(3),
            probe
                .0
                .connect(Credentials::with_access_token(token), false),
        )
        .await
        .map_err(|_| ServiceFailure::spotify(FailureKind::Transport))?
        .map_err(|_| ServiceFailure::spotify(FailureKind::AuthenticationRequired))?;
        streaming
            .verify_streaming_username(&probe.0.username())
            .await?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match probe.0.get_user_attribute("type").as_deref() {
                Some("premium") => return Ok(Some(AccountPlan::Premium)),
                Some("free" | "open") => return Ok(Some(AccountPlan::Free)),
                _ if Instant::now() >= deadline => return Ok(None),
                _ => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    }

    pub(crate) async fn account_plan(&self) -> Result<AccountPlan> {
        self.account_plan_at("https://api.spotify.com/v1/me").await
    }

    async fn account_plan_at(&self, endpoint: &str) -> Result<AccountPlan> {
        let mut token = self.access().await?;
        for attempt in 0..2 {
            let response = self
                .client
                .get(endpoint)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|_| ServiceFailure::spotify(FailureKind::Transport))?;
            if response.status().as_u16() == 401 && attempt == 0 {
                token = self.refresh_rejected(&token).await?;
                continue;
            }
            if response.status().as_u16() != 200 {
                return Err(ServiceFailure::from_response(response, Provider::Spotify)
                    .await
                    .into());
            }
            let profile: serde_json::Value = response
                .json()
                .await
                .map_err(|_| ServiceFailure::spotify(FailureKind::InvalidResponse))?;
            let verified = AccountIdentity::from_profile(&profile)
                .map_err(|_| ServiceFailure::spotify(FailureKind::InvalidResponse))?;
            let saved = token_identity(&*self.state.lock().await, false)?;
            if known_account_id(&saved.stable_web_api_id).is_some()
                || known_account_id(&saved.legacy_web_api_id).is_some()
            {
                require_same(saved.relation(&verified))?;
            }
            return AccountPlan::from_profile(&profile);
        }
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    #[tokio::test]
    #[ignore = "Read-only eligibility and latency check using the saved Spotify account and live network"]
    async fn live_saved_spotify_plan_latency() {
        let store = Storage::local_read_only().unwrap();
        let Some(tokens) = TokenManager::load_optional(&store.config().unwrap()).unwrap() else {
            println!("No saved Spotify account: live Premium playback cannot be validated.");
            return;
        };
        let begin = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(6), tokens.account_plan()).await;
        match result {
            Ok(Ok(plan)) => {
                println!(
                    "Live Spotify plan: {plan:?}; profile {:.3}s",
                    begin.elapsed().as_secs_f64()
                );
                if plan == AccountPlan::Unavailable {
                    let begin = Instant::now();
                    let plan =
                        tokio::time::timeout(Duration::from_secs(4), tokens.streaming_plan()).await;
                    match plan {
                        Ok(Ok(plan)) => println!(
                            "Streaming entitlement: {plan:?}; {:.3}s",
                            begin.elapsed().as_secs_f64()
                        ),
                        _ => println!(
                            "Streaming entitlement could not be confirmed on this connection."
                        ),
                    }
                }
            }
            Ok(Err(error)) => println!(
                "Live Spotify plan check unavailable: {error}; {:.3}s",
                begin.elapsed().as_secs_f64()
            ),
            Err(_) => println!("Live Spotify plan check timed out."),
        }
    }

    #[tokio::test]
    async fn profile_plan_distinguishes_premium_and_free_without_streaming() {
        for (product, expected) in [
            ("premium", AccountPlan::Premium),
            ("free", AccountPlan::Free),
            ("open", AccountPlan::Free),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/me"))
                .and(header("authorization", "Bearer old"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"id":"account", "product":product})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let tokens = TokenManager::mock_for_account(
                format!("{}/token", server.uri()),
                "client",
                "account",
            );
            assert_eq!(
                tokens
                    .account_plan_at(&format!("{}/me", server.uri()))
                    .await
                    .unwrap(),
                expected
            );
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn unknown_and_mismatched_profiles_cannot_select_youtube() {
        for profile in [
            json!({"id":"account", "product":"unknown"}),
            json!({"id":"account", "product":123}),
            json!({"product":"free"}),
            json!({"id":"other", "product":"free"}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(profile))
                .expect(1)
                .mount(&server)
                .await;
            let tokens = TokenManager::mock_for_account(server.uri(), "client", "account");
            assert!(tokens.account_plan_at(&server.uri()).await.is_err());
            assert_eq!(tokens.state.lock().await.account_id, "account");
        }
    }

    #[tokio::test]
    async fn omitted_subscription_is_unavailable_rather_than_free() {
        for profile in [
            json!({"id":"account"}),
            json!({"id":"account", "product":null}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(profile))
                .expect(1)
                .mount(&server)
                .await;
            let tokens = TokenManager::mock_for_account(server.uri(), "client", "account");
            assert_eq!(
                tokens.account_plan_at(&server.uri()).await.unwrap(),
                AccountPlan::Unavailable
            );
        }
    }

    #[tokio::test]
    async fn profile_failures_keep_their_cause_and_do_not_refresh_or_fallback() {
        for (status, kind) in [
            (403, FailureKind::AccessRestricted),
            (429, FailureKind::RateLimited),
            (503, FailureKind::Server),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;
            let tokens = TokenManager::mock(server.uri(), false);
            let error = tokens.account_plan_at(&server.uri()).await.unwrap_err();
            assert!(ServiceFailure::is(&error, kind));
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn rejected_profile_token_refreshes_once_then_reads_the_plan() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .and(header("authorization", "Bearer old"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"access_token":"new", "expires_in":3600})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me"))
            .and(header("authorization", "Bearer new"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id":"account", "product":"premium"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let tokens = TokenManager::mock(format!("{}/token", server.uri()), false);
        assert_eq!(
            tokens
                .account_plan_at(&format!("{}/me", server.uri()))
                .await
                .unwrap(),
            AccountPlan::Premium
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }
}
