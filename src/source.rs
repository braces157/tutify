//! Launch policy is separate from the concrete provider stored in a running app.
use crate::{
    auth::{AccountPlan, TokenManager},
    model::MusicSource,
    storage::Storage,
};
use anyhow::Result;
use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Choice {
    #[default]
    Auto,
    Spotify,
    Youtube,
}

pub struct Selection {
    pub source: MusicSource,
    pub premium_verified: bool,
    pub notice: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PlanMemo {
    version: u32,
    account: String,
    plan: AccountPlan,
    checked_at: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl PlanMemo {
    fn read(store: &Storage, account: &str, at: u64) -> Option<AccountPlan> {
        let bytes = std::fs::read(store.root.join("launch-plan.json")).ok()?;
        if bytes.len() > 4096 {
            return None;
        }
        let memo: Self = serde_json::from_slice(&bytes).ok()?;
        (memo.version == 1
            && memo.account == account
            && memo.checked_at <= at
            && at - memo.checked_at < 10 * 60
            && memo.plan != AccountPlan::Unavailable)
            .then_some(memo.plan)
    }
}
pub(crate) async fn remember_free(store: &Storage) {
    if let Ok(config) = store.config()
        && let Ok(Some(tokens)) = TokenManager::load_optional(&config)
        && let Ok(account) = tokens.account_key().await
    {
        let _ = store.save_launch_plan(&PlanMemo {
            version: 1,
            account,
            plan: AccountPlan::Free,
            checked_at: now(),
        });
    }
}
pub async fn resolve(choice: Choice, fresh: bool) -> Result<Selection> {
    resolve_with(choice, || async {
        let store = Storage::local_read_only()?;
        let config = store.config()?;
        let Some(tokens) = TokenManager::load_optional(&config)? else {
            return Ok(None);
        };
        let account = tokens.account_key().await?;
        if !fresh && let Some(plan) = PlanMemo::read(&store, &account, now()) {
            return Ok(Some(plan));
        }
        let mut plan = tokens.account_plan().await?;
        if plan == AccountPlan::Unavailable {
            plan = tokens
                .streaming_plan()
                .await?
                .unwrap_or(AccountPlan::Unavailable);
        }
        if plan != AccountPlan::Unavailable {
            let _ = store.save_launch_plan(&PlanMemo {
                version: 1,
                account,
                plan,
                checked_at: now(),
            });
        }
        Ok(Some(plan))
    })
    .await
}

async fn resolve_with<F, Fut>(choice: Choice, account: F) -> Result<Selection>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Option<AccountPlan>>>,
{
    let (source, premium_verified, notice) = match choice {
        Choice::Spotify => (MusicSource::Spotify, false, ""),
        Choice::Youtube => (MusicSource::Youtube, false, ""),
        Choice::Auto => match tokio::time::timeout(Duration::from_secs(3), account()).await {
            Ok(Ok(plan)) => match plan {
                Some(AccountPlan::Premium) => (
                    MusicSource::Spotify,
                    true,
                    "Spotify Premium: opening Spotify.",
                ),
                Some(AccountPlan::Free) => (
                    MusicSource::Youtube,
                    false,
                    "Spotify Free: music is ready. F6 connects your library.",
                ),
                Some(AccountPlan::Unavailable) => (
                    MusicSource::Youtube,
                    false,
                    "Spotify could not confirm Premium; free music is ready. F6 reconnects your account.",
                ),
                None => (
                    MusicSource::Youtube,
                    false,
                    "Music is ready. F6 connects Spotify or your Google library.",
                ),
            },
            Ok(Err(error)) => {
                return Ok(Selection {
                    source: MusicSource::Youtube,
                    premium_verified: false,
                    notice: format!(
                        "Spotify check unavailable ({error}); free music is ready. F6 reconnects your account."
                    ),
                });
            }
            Err(_) => (
                MusicSource::Youtube,
                false,
                "Spotify check timed out; free music is ready. F6 reconnects your account.",
            ),
        },
    };
    Ok(Selection {
        source,
        premium_verified,
        notice: notice.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn automatic_launch_routes_premium_free_and_unsigned_users() {
        for (plan, expected) in [
            (Some(AccountPlan::Premium), MusicSource::Spotify),
            (Some(AccountPlan::Free), MusicSource::Youtube),
            (Some(AccountPlan::Unavailable), MusicSource::Youtube),
            (None, MusicSource::Youtube),
        ] {
            let selected = resolve_with(Choice::Auto, || async { Ok(plan) })
                .await
                .unwrap();
            assert_eq!(selected.source, expected);
            assert_eq!(
                selected.premium_verified,
                plan == Some(AccountPlan::Premium)
            );
            assert!(!selected.notice.is_empty());
        }
    }

    #[tokio::test]
    async fn explicit_sources_never_inspect_spotify_credentials() {
        for (choice, expected) in [
            (Choice::Spotify, MusicSource::Spotify),
            (Choice::Youtube, MusicSource::Youtube),
        ] {
            let selected = resolve_with(choice, || async {
                panic!("explicit choice must bypass detection")
            })
            .await
            .unwrap();
            assert_eq!(selected.source, expected);
            assert!(!selected.premium_verified);
        }
    }

    #[tokio::test]
    async fn account_failures_never_silently_change_provider() {
        use crate::service::{FailureKind, ServiceFailure};
        for kind in [
            FailureKind::AuthenticationRequired,
            FailureKind::AccessRestricted,
            FailureKind::RateLimited,
            FailureKind::QuotaExceeded,
            FailureKind::Server,
            FailureKind::Transport,
            FailureKind::InvalidResponse,
        ] {
            let selected = resolve_with(Choice::Auto, || async {
                Err(ServiceFailure::spotify(kind).into())
            })
            .await
            .unwrap();
            assert_eq!(selected.source, MusicSource::Youtube);
            assert!(selected.notice.contains("unavailable"));
            assert!(!selected.notice.contains("--source"));
        }
        let selected = resolve_with(Choice::Auto, || async {
            Ok(Some(AccountPlan::Unavailable))
        })
        .await
        .unwrap();
        assert_eq!(selected.source, MusicSource::Youtube);
        assert!(!selected.notice.contains("--source"));
    }
    #[tokio::test]
    async fn hung_account_request_is_bounded_and_cancelled() {
        let begin = std::time::Instant::now();
        let selected = resolve_with(Choice::Auto, std::future::pending)
            .await
            .unwrap();
        assert_eq!(selected.source, MusicSource::Youtube);
        assert!(selected.notice.contains("timed out"));
        assert!(begin.elapsed() < Duration::from_secs(4));
    }
    #[test]
    fn memo_is_account_scoped_expires_and_preserves_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().into(),
        };
        store
            .save_launch_plan(&PlanMemo {
                version: 1,
                account: "one".into(),
                plan: AccountPlan::Premium,
                checked_at: 1000,
            })
            .unwrap();
        assert_eq!(
            PlanMemo::read(&store, "one", 1100),
            Some(AccountPlan::Premium)
        );
        assert_eq!(PlanMemo::read(&store, "two", 1100), None);
        assert_eq!(PlanMemo::read(&store, "one", 1600), None);
        assert_eq!(PlanMemo::read(&store, "one", 999), None);
        std::fs::write(store.root.join("launch-plan.json"), b"preserve me").unwrap();
        assert_eq!(PlanMemo::read(&store, "one", 1100), None);
        assert_eq!(
            std::fs::read(store.root.join("launch-plan.json")).unwrap(),
            b"preserve me"
        );
    }
}
