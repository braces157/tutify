//! Safe, typed service failures. Never retain an upstream body, URL or request error.
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Spotify,
    SimilarArtists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    AuthenticationRequired,
    AccessRestricted,
    MissingItem,
    RateLimited,
    QuotaExceeded,
    Server,
    Transport,
    InvalidResponse,
    RequestRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceFailure {
    pub provider: Provider,
    pub kind: FailureKind,
    pub status: Option<u16>,
    pub retry_at: Option<Instant>,
}

impl ServiceFailure {
    pub fn new(provider: Provider, kind: FailureKind) -> Self {
        Self {
            provider,
            kind,
            status: None,
            retry_at: None,
        }
    }

    pub fn spotify(kind: FailureKind) -> Self {
        Self::new(Provider::Spotify, kind)
    }

    pub fn is(error: &anyhow::Error, kind: FailureKind) -> bool {
        error
            .downcast_ref::<Self>()
            .is_some_and(|failure| failure.kind == kind)
    }

    pub fn throttled(self) -> bool {
        matches!(
            self.kind,
            FailureKind::RateLimited | FailureKind::QuotaExceeded
        )
    }

    pub fn active(self) -> bool {
        self.retry_at
            .map_or_else(|| self.throttled(), |until| until > Instant::now())
    }

    pub async fn from_response(mut response: reqwest::Response, provider: Provider) -> Self {
        let status = response.status().as_u16();
        let delay = retry_after(
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
        );
        let mut kind = match status {
            401 => FailureKind::AuthenticationRequired,
            403 => FailureKind::AccessRestricted,
            404 => FailureKind::MissingItem,
            429 => FailureKind::RateLimited,
            500..=599 => FailureKind::Server,
            _ => FailureKind::RequestRejected,
        };
        if status == 429 {
            // Stop reading after 16 KiB. Oversized/malformed bodies keep HTTP classification.
            let mut body = Vec::new();
            let mut complete = true;
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) if body.len() + chunk.len() <= 16 * 1024 => {
                        body.extend_from_slice(&chunk)
                    }
                    Ok(None) => break,
                    _ => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body)
                && value["error"]["reason"].as_str() == Some("QUOTA_EXCEEDED")
            {
                kind = FailureKind::QuotaExceeded;
            }
        }
        // An ordinary missing Retry-After uses the existing conservative cooldown.
        // Exhausted quota has no invented reset: keep it blocked for this client session.
        let delay = if kind == FailureKind::RateLimited {
            Some(delay.unwrap_or(Duration::from_secs(60)))
        } else if kind == FailureKind::QuotaExceeded {
            delay
        } else {
            None
        };
        Self {
            provider,
            kind,
            status: Some(status),
            retry_at: delay.and_then(|wait| Instant::now().checked_add(wait)),
        }
    }
}

pub fn retry_after(header: Option<&str>) -> Option<Duration> {
    header.and_then(|s| {
        s.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
            httpdate::parse_http_date(s)
                .ok()
                .map(|date| date.duration_since(SystemTime::now()).unwrap_or_default())
        })
    })
}

fn remaining_seconds(until: Instant) -> u64 {
    let remaining = until.saturating_duration_since(Instant::now());
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

impl std::fmt::Display for ServiceFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let provider = match self.provider {
            Provider::Spotify => "Spotify",
            Provider::SimilarArtists => "Similar-artist service",
        };
        match self.kind {
            FailureKind::AuthenticationRequired => write!(
                f,
                "{provider} login expired or was revoked; run tuitify auth --force"
            ),
            FailureKind::AccessRestricted if self.status == Some(200) => write!(
                f,
                "{provider} returned playlist metadata without access to its contents. Owners or collaborators may have access; choose another playlist."
            ),
            FailureKind::AccessRestricted => write!(
                f,
                "{provider} denied access (HTTP 403). Check that this account is allowed in your Developer app, its scopes and the app owner's Premium subscription. Restricted playlist items can require ownership or collaboration."
            ),
            FailureKind::MissingItem => write!(
                f,
                "{provider} item not available to this account. Choose another item."
            ),
            FailureKind::RateLimited => {
                let seconds = self.retry_at.map_or(60, remaining_seconds);
                write!(
                    f,
                    "{provider} rate limit (HTTP 429): wait at least {seconds} seconds, then retry with F5. Premium is not the issue; avoid repeated login attempts."
                )
            }
            FailureKind::QuotaExceeded => {
                write!(
                    f,
                    "{provider} quota exhausted (QUOTA_EXCEEDED). Logging in again will not replenish quota. Recovery time is unknown."
                )?;
                if let Some(until) = self.retry_at {
                    write!(
                        f,
                        " Wait at least {} seconds before another attempt; this does not guarantee quota recovery.",
                        remaining_seconds(until)
                    )
                } else {
                    f.write_str(" Requests are blocked for this client session; reopen Tuitify when quota is available.")
                }
            }
            FailureKind::Server if self.status == Some(200) => {
                write!(f, "{provider} reported a service error; retry later.")
            }
            FailureKind::Server => write!(
                f,
                "{provider} returned HTTP {}. Retry later with F5; your queue is preserved.",
                self.status.unwrap_or(500)
            ),
            FailureKind::Transport => write!(
                f,
                "Cannot reach {provider}. Check your connection, then retry; your queue is preserved."
            ),
            FailureKind::InvalidResponse => {
                write!(f, "{provider} returned an invalid response; retry later.")
            }
            FailureKind::RequestRejected => write!(
                f,
                "{provider} rejected the request (HTTP {}). Check app access and retry later.",
                self.status.unwrap_or(400)
            ),
        }
    }
}
impl std::error::Error for ServiceFailure {}

#[cfg(test)]
mod tests;
