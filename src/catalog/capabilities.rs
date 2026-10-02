//! Session observations belong to one Catalog/TokenManager identity, never to
//! a global Spotify profile. A bare 403 cannot prove an endpoint was removed.
use crate::service::{FailureKind, ServiceFailure};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;

const DENIAL_TTL: Duration = Duration::from_secs(5 * 60);
const SUPPORT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_OBSERVATIONS: usize = 256;

/// Marks a response from the requested catalog endpoint. A login service's
/// 403 is not evidence that an artist/playlist/recommendations endpoint failed.
#[derive(Debug)]
pub(super) struct Denied;

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Catalog access is unavailable for this item")
    }
}
impl std::error::Error for Denied {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Endpoint {
    ArtistTopTracks,
    Recommendations,
    PlaylistItems,
    SaveLibrary,
    RemoveLibrary,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub endpoint: Endpoint,
    resource: String,
}

impl Key {
    pub fn expected_shape(&self, value: &serde_json::Value) -> bool {
        match self.endpoint {
            Endpoint::ArtistTopTracks | Endpoint::Recommendations => value["tracks"].is_array(),
            Endpoint::PlaylistItems => value["items"].is_array(),
            Endpoint::SaveLibrary | Endpoint::RemoveLibrary => true,
        }
    }

    pub fn invalid_response_hint(&self) -> &'static str {
        if self.endpoint == Endpoint::PlaylistItems {
            "Spotify omitted playlist items; content access can require owners or collaborators. Press F5 to recheck"
        } else {
            "Spotify omitted catalog items; press F5 to retry"
        }
    }

    pub fn metadata_only_playlist(&self, value: &serde_json::Value) -> bool {
        self.endpoint == Endpoint::PlaylistItems
            && value["id"].as_str() == Some(self.resource.as_str())
            && value["type"].as_str() == Some("playlist")
            && value["name"].as_str().is_some()
            && value.get("items").is_none()
            && value.get("tracks").is_none()
    }

    pub fn for_request(method: &str, path: &str, query: &[(&str, String)]) -> Option<Self> {
        let parts: Vec<_> = path.split('/').collect();
        let (endpoint, resource) = match (method, parts.as_slice()) {
            ("GET", ["", "artists", id, "top-tracks"]) => {
                (Endpoint::ArtistTopTracks, (*id).to_owned())
            }
            ("GET", ["", "playlists", id, "items"]) => (Endpoint::PlaylistItems, (*id).to_owned()),
            ("GET", ["", "recommendations"]) => {
                // A seed can be restricted without establishing a global denial.
                let mut seeds: Vec<_> = query
                    .iter()
                    .filter(|(key, _)| key.starts_with("seed_"))
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                seeds.sort();
                (Endpoint::Recommendations, seeds.join("&"))
            }
            // Register observations when optional library writes are introduced.
            // No write request or additional OAuth scope is enabled here.
            ("PUT", ["", "me", "library"]) => (
                Endpoint::SaveLibrary,
                query
                    .iter()
                    .find(|(key, _)| *key == "uris")
                    .map_or_else(String::new, |(_, value)| value.clone()),
            ),
            ("DELETE", ["", "me", "library"]) => (
                Endpoint::RemoveLibrary,
                query
                    .iter()
                    .find(|(key, _)| *key == "uris")
                    .map_or_else(String::new, |(_, value)| value.clone()),
            ),
            _ => return None,
        };
        Some(Self { endpoint, resource })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Supported,
    Denied(ServiceFailure),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Observation {
    outcome: Outcome,
    expires_at: Instant,
}

impl Observation {
    pub fn from_result(result: &anyhow::Result<serde_json::Value>) -> Option<Self> {
        let (outcome, ttl) = match result {
            Ok(_) => (Outcome::Supported, SUPPORT_TTL),
            Err(error) => {
                if !error.is::<Denied>() {
                    return None;
                }
                let failure = *error.downcast_ref::<ServiceFailure>()?;
                if !matches!(
                    failure.kind,
                    FailureKind::AccessRestricted | FailureKind::MissingItem
                ) {
                    return None;
                }
                (Outcome::Denied(failure), DENIAL_TTL)
            }
        };
        Some(Self {
            outcome,
            expires_at: Instant::now() + ttl,
        })
    }

    pub fn outcome(self) -> Option<Outcome> {
        (self.expires_at > Instant::now()).then_some(self.outcome)
    }

    pub fn denial(self) -> Option<ServiceFailure> {
        match self.outcome() {
            Some(Outcome::Denied(failure)) => Some(failure),
            _ => None,
        }
    }
}

pub(super) type Slot = Arc<AsyncMutex<Option<Observation>>>;

#[derive(Default)]
pub(super) struct Capabilities {
    slots: Mutex<HashMap<Key, (Instant, Slot)>>,
}

impl Capabilities {
    pub fn summary(&self) -> Vec<super::CapabilitySummary> {
        let slots: Vec<_> = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(key, (_, slot))| (key.endpoint, slot.clone()))
            .collect();
        let mut summaries = super::CapabilitySummary::unknown();
        for (endpoint, slot) in slots {
            let index = match endpoint {
                Endpoint::ArtistTopTracks => 0,
                Endpoint::Recommendations => 1,
                Endpoint::PlaylistItems => 2,
                Endpoint::SaveLibrary => 3,
                Endpoint::RemoveLibrary => 4,
            };
            let summary = &mut summaries[index];
            match slot
                .try_lock()
                .ok()
                .and_then(|guard| guard.and_then(Observation::outcome))
            {
                Some(Outcome::Supported) => summary.supported += 1,
                Some(Outcome::Denied(failure)) if failure.kind == FailureKind::MissingItem => {
                    summary.missing += 1
                }
                Some(Outcome::Denied(_)) => summary.restricted += 1,
                None => summary.unknown += 1,
            }
        }
        summaries
    }
    pub fn slot(&self, key: Key) -> Slot {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, slot)) = slots.get(&key) {
            return slot.clone();
        }
        if slots.len() >= MAX_OBSERVATIONS
            && let Some(oldest) = slots
                .iter()
                .min_by_key(|(_, (created, _))| *created)
                .map(|(key, _)| key.clone())
        {
            slots.remove(&oldest);
        }
        let slot = Arc::new(AsyncMutex::new(None));
        slots.insert(key, (Instant::now(), slot.clone()));
        slot
    }

    pub fn refresh(&self) {
        // Detach outstanding slots too. A pre-refresh HTTP response cannot
        // install an old denial into the refreshed observation map.
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn support_summary_is_resource_free_bounded_and_marks_pending_expired_unknown() {
        let capabilities = Capabilities::default();
        let key =
            |id: &str| Key::for_request("GET", &format!("/playlists/{id}/items"), &[]).unwrap();
        let supported = capabilities.slot(key("private-owned-playlist"));
        *supported.try_lock().unwrap() =
            Observation::from_result(&Ok(serde_json::json!({"items":[]})));
        let denied = capabilities.slot(key("private-collaborator-playlist"));
        let failure = ServiceFailure::spotify(FailureKind::AccessRestricted);
        *denied.try_lock().unwrap() =
            Observation::from_result(&Err(anyhow::Error::new(failure).context(Denied)));
        let pending = capabilities.slot(key("private-pending-playlist"));
        let _held = pending.try_lock().unwrap();
        let expired = capabilities.slot(key("private-expired-playlist"));
        *expired.try_lock().unwrap() = Some(Observation {
            outcome: Outcome::Supported,
            expires_at: Instant::now() - Duration::from_secs(1),
        });
        let summary = capabilities.summary();
        assert_eq!(summary[2].supported, 1);
        assert_eq!(summary[2].restricted, 1);
        assert_eq!(summary[2].unknown, 2);
        let text = serde_json::to_string(&summary).unwrap();
        assert!(!text.contains("private-"));
        assert_eq!(summary[0].supported, 0);
        capabilities.refresh();
        assert!(capabilities.summary().iter().all(|item| item.supported
            + item.restricted
            + item.missing
            + item.unknown
            == 0));
    }

    #[tokio::test]
    async fn observations_expire_and_refresh_detaches_pending_results() {
        let key = Key::for_request("GET", "/playlists/id/items", &[]).unwrap();
        let capabilities = Capabilities::default();
        let pending = capabilities.slot(key.clone());
        capabilities.refresh();
        let new = capabilities.slot(key);
        assert!(!Arc::ptr_eq(&pending, &new));
        let failure = ServiceFailure::spotify(FailureKind::AccessRestricted);
        let mut observation =
            Observation::from_result(&Err(anyhow::Error::new(failure).context(Denied))).unwrap();
        assert_eq!(observation.denial(), Some(failure));
        observation.expires_at = Instant::now() - Duration::from_secs(1);
        assert_eq!(observation.denial(), None);
        assert_eq!(observation.outcome(), None);
        *pending.lock().await = Some(observation);
        assert!(new.lock().await.is_none());
        let mut observation = Observation::from_result(&Ok(serde_json::json!({}))).unwrap();
        assert_eq!(observation.outcome, Outcome::Supported);
        assert_eq!(observation.outcome(), Some(Outcome::Supported));
        assert_eq!(observation.denial(), None);
        assert!(observation.expires_at <= Instant::now() + SUPPORT_TTL);
        observation.expires_at = Instant::now() - Duration::from_secs(1);
        assert_eq!(observation.outcome(), None);
    }

    #[test]
    fn resource_and_write_action_keys_are_distinct_and_capacity_is_bounded() {
        let save = Key::for_request("PUT", "/me/library", &[]).unwrap();
        let remove = Key::for_request("DELETE", "/me/library", &[]).unwrap();
        assert_ne!(save, remove);
        assert!(Key::for_request("GET", "/me/library", &[]).is_none());
        let capabilities = Capabilities::default();
        for id in 0..MAX_OBSERVATIONS * 2 {
            capabilities
                .slot(Key::for_request("GET", &format!("/playlists/{id}/items"), &[]).unwrap());
        }
        assert_eq!(capabilities.slots.lock().unwrap().len(), MAX_OBSERVATIONS);
    }
}
