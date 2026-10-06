//! A bounded, owned session journal. Records cannot contain upstream text.
use crate::service::{FailureKind, Provider, ServiceFailure};
use serde::Serialize;
use std::{collections::VecDeque, time::Instant};

pub(crate) const MAX_ERRORS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Subsystem {
    Catalog,
    Library,
    Metadata,
    Lyrics,
    Recommendations,
    Mix,
    Playback,
    Storage,
    Terminal,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "kind")]
pub(crate) enum Cause {
    Service(FailureKind),
    Local,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ErrorRecord {
    pub sequence: u64,
    pub elapsed_seconds: u64,
    pub subsystem: Subsystem,
    pub cause: Cause,
    pub provider: Option<Provider>,
    pub http_status: Option<u16>,
    pub retry_after_seconds: Option<u64>,
}
impl ErrorRecord {
    pub fn summary(&self) -> String {
        match self.cause {
            Cause::Service(kind) => format!(
                "{:?}: {kind:?}{}",
                self.subsystem,
                self.http_status
                    .map_or_else(String::new, |status| format!(" (HTTP {status})"))
            ),
            Cause::Local => format!(
                "{:?}: local or unclassified failure (details omitted)",
                self.subsystem
            ),
        }
    }
    pub fn action(&self) -> &'static str {
        match self.cause {
            Cause::Service(FailureKind::AuthenticationRequired) => {
                if self.provider == Some(Provider::YoutubeMusic) {
                    "F6 > Connect Google music library. Public search remains available."
                } else {
                    "F6 > Connect Spotify account to renew the login."
                }
            }
            Cause::Service(FailureKind::AccessRestricted | FailureKind::MissingItem) => {
                "Check account/resource access or choose another item; F5 rechecks scoped access."
            }
            Cause::Service(FailureKind::RateLimited) => {
                "Wait for the reported minimum delay before retrying. Avoid repeated authentication."
            }
            Cause::Service(FailureKind::QuotaExceeded) => {
                "Quota exhaustion is separate from a short rate limit. Logging in again does not replenish quota; recovery time may be unknown."
            }
            Cause::Service(_) => {
                "Check connectivity or retry later with F5. Preserve the support report if the failure repeats."
            }
            Cause::Local if self.subsystem == Subsystem::Storage => {
                "Run tuitify doctor and state inspect; preserve originals before previewing targeted recovery."
            }
            Cause::Local if self.subsystem == Subsystem::Playback => {
                "Check doctor output and Windows audio settings; restart Tuitify if its playback worker stopped."
            }
            Cause::Local => {
                "Inspect the current on-screen error locally and run tuitify doctor. Sensitive upstream details are omitted from this report."
            }
        }
    }
}

pub(crate) struct History {
    started: Instant,
    sequence: u64,
    records: VecDeque<ErrorRecord>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            sequence: 0,
            records: VecDeque::new(),
        }
    }
}
impl History {
    fn push(&mut self, subsystem: Subsystem, failure: Option<ServiceFailure>) {
        self.sequence = self.sequence.saturating_add(1);
        if self.records.len() == MAX_ERRORS {
            self.records.pop_front();
        }
        self.records.push_back(ErrorRecord {
            sequence: self.sequence,
            elapsed_seconds: self.started.elapsed().as_secs(),
            subsystem,
            cause: failure.map_or(Cause::Local, |failure| Cause::Service(failure.kind)),
            provider: failure.map(|failure| failure.provider),
            http_status: failure.and_then(|failure| failure.status),
            retry_after_seconds: failure.and_then(|failure| failure.retry_at).map(|until| {
                let remaining = until.saturating_duration_since(Instant::now());
                remaining
                    .as_secs()
                    .saturating_add(u64::from(remaining.subsec_nanos() > 0))
            }),
        });
    }
    pub fn record(&mut self, subsystem: Subsystem, error: &anyhow::Error) {
        self.push(subsystem, error.downcast_ref::<ServiceFailure>().copied());
    }
    /// Compatibility for background messages that already lost their error type.
    /// Recognize only our static service phrases; never retain raw strings/paths.
    pub fn record_text(&mut self, subsystem: Subsystem, text: &str) {
        let kind = if text.contains("quota exhausted (QUOTA_EXCEEDED)") {
            Some((FailureKind::QuotaExceeded, Some(429)))
        } else if text.contains("rate limit (HTTP 429)")
            || text.contains("rate limiting this connection (HTTP 429)")
        {
            Some((FailureKind::RateLimited, Some(429)))
        } else if text.contains("HTTP 403") {
            Some((FailureKind::AccessRestricted, Some(403)))
        } else if text.contains("HTTP 401")
            || text.contains("login expired or was revoked")
            || text.contains("Google music library connection is missing or expired")
        {
            Some((FailureKind::AuthenticationRequired, Some(401)))
        } else if text.contains("HTTP 404") {
            Some((FailureKind::MissingItem, Some(404)))
        } else {
            None
        };
        let failure = kind.map(|(kind, status)| ServiceFailure {
            provider: if text.contains("Similar-artist service") {
                Provider::SimilarArtists
            } else if text.contains("YouTube") || text.contains("Google music") {
                Provider::YoutubeMusic
            } else {
                Provider::Spotify
            },
            kind,
            status,
            retry_at: None,
        });
        self.push(subsystem, failure);
    }
    pub fn records(&self) -> &VecDeque<ErrorRecord> {
        &self.records
    }
    pub fn snapshot(&self) -> Vec<ErrorRecord> {
        self.records.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_music_failures_keep_their_provider_and_use_in_app_recovery() {
        let mut history = History::default();
        history.record_text(
            Subsystem::Playback,
            "YouTube is rate limiting this connection (HTTP 429). Wait before retrying.",
        );
        let rate = history.records().back().unwrap();
        assert_eq!(rate.provider, Some(Provider::YoutubeMusic));
        assert_eq!(rate.cause, Cause::Service(FailureKind::RateLimited));
        history.record_text(
            Subsystem::Library,
            "Google music library connection is missing or expired. F6 connects it.",
        );
        let auth = history.records().back().unwrap();
        assert_eq!(auth.provider, Some(Provider::YoutubeMusic));
        assert_eq!(
            auth.cause,
            Cause::Service(FailureKind::AuthenticationRequired)
        );
        assert!(auth.action().contains("Connect Google music library"));
        assert!(!auth.action().contains("tuitify youtube"));
    }
    #[test]
    fn bounded_session_records_keep_classification_and_never_retain_context() {
        let mut history = History::default();
        let error = anyhow::Error::new(ServiceFailure {
            provider: Provider::Spotify, kind: FailureKind::QuotaExceeded, status: Some(429),
            retry_at: Instant::now().checked_add(std::time::Duration::from_secs(60)),
        }).context("Bearer private-fixture-secret http://127.0.0.1:8989/callback?code=private-code C:\\Users\\Private Person\\song-history.json");
        for _ in 0..MAX_ERRORS + 5 {
            history.record(Subsystem::Catalog, &error);
        }
        assert_eq!(history.records().len(), MAX_ERRORS);
        assert_eq!(history.records().front().unwrap().sequence, 6);
        assert_eq!(history.records().back().unwrap().sequence, 69);
        let record = history.records().back().unwrap();
        assert_eq!(record.http_status, Some(429));
        assert_eq!(record.cause, Cause::Service(FailureKind::QuotaExceeded));
        assert!(record.retry_after_seconds.is_some());
        assert!(record.action().contains("does not replenish quota"));
        let exported = serde_json::to_string(&history.snapshot()).unwrap();
        for secret in [
            "private-fixture-secret",
            "private-code",
            "Private Person",
            "song-history.json",
            "http://",
        ] {
            assert!(!exported.contains(secret));
        }
        assert!(History::default().records().is_empty());
    }
    #[test]
    fn compatibility_strings_classify_safe_phrases_and_omit_everything_else() {
        let mut history = History::default();
        history.record_text(
            Subsystem::Mix,
            "private song name: Spotify denied access (HTTP 403). token=private-token",
        );
        history.record_text(
            Subsystem::Storage,
            "C:\\Users\\Sensitive\\backup failed with private raw JSON",
        );
        assert_eq!(
            history.records()[0].cause,
            Cause::Service(FailureKind::AccessRestricted)
        );
        assert_eq!(history.records()[1].cause, Cause::Local);
        let text = serde_json::to_string(&history.snapshot()).unwrap();
        for secret in ["private song", "private-token", "Sensitive", "raw JSON"] {
            assert!(!text.contains(secret));
        }
    }
}
