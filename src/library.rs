//! Streaming search across the account's saved Spotify library.
//!
//! The search deliberately uses the catalog's page API rather than creating a
//! task for every page.  Apart from keeping Spotify request volume predictable,
//! this means dropping/aborting the future also drops the request that is in
//! flight; there are no detached workers to finish after cancellation.

use crate::{
    catalog::{Browse, Catalog, Rows},
    model::Track,
    service::FailureKind,
};
use anyhow::{Context, Result, anyhow, bail};
use std::{collections::HashSet, time::Duration};
use tokio::sync::mpsc::Sender;

/// Keep this limit in sync with the queue's persistence limit.
pub use crate::queue::MAX_TRACKS;

/// Maximum number of catalog page requests in one search.
///
/// A well-behaved Spotify response finishes before this limit because every
/// page is bounded by the queue's track limit.  The separate page cap prevents
/// a malformed or changing `next` response that contains no rows from causing
/// an unbounded scan.
pub const MAX_PAGE_REQUESTS: usize = MAX_TRACKS;

#[cfg(not(test))]
const PAGE_DELAY: Duration = Duration::from_millis(200);
#[cfg(test)]
const PAGE_DELAY: Duration = Duration::ZERO;

/// A delta emitted while a saved-library search is running.
///
/// `tracks` contains only matches discovered since the previous progress
/// message.  `scanned` is cumulative and counts valid track rows, including
/// rows whose IDs were already seen in another liked-song or playlist page.
/// `skipped` contains newly inaccessible playlists. A successful traversal
/// is only complete when no playlist sources were skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryProgress {
    pub tracks: Vec<Track>,
    pub scanned: usize,
    pub complete: bool,
    pub skipped: Vec<SkippedPlaylist>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistSkipReason {
    Restricted,
    Missing,
}
impl PlaylistSkipReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Restricted => "Access restricted",
            Self::Missing => "Not found / inaccessible",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedPlaylist {
    pub id: String,
    pub name: String,
    pub reason: PlaylistSkipReason,
}

/// Search all liked songs and all saved playlists for `query`.
///
/// The search sends progress as pages are consumed.  A dropped receiver is
/// treated as cancellation and returns an error.  Callers can retain every
/// delta received before an HTTP or traversal error and use the final
/// `complete` flag to distinguish a complete result from a partial one.
pub async fn search(catalog: Catalog, query: String, tx: Sender<LibraryProgress>) -> Result<()> {
    let mut scan = Scanner::new(query, tx);
    let result = scan.run(&catalog).await;
    match result {
        Ok(()) => scan.emit(Vec::new(), true).await,
        Err(error) => {
            // Best effort: preserve the original catalog/traversal error.  If
            // the receiver was closed, its sender error is less useful than
            // the HTTP error which caused the partial result.
            let _ = scan.emit(Vec::new(), false).await;
            Err(error)
        }
    }
}

struct Scanner {
    query: String,
    tx: Sender<LibraryProgress>,
    seen_tracks: HashSet<String>,
    seen_playlists: HashSet<String>,
    scanned: usize,
    page_requests: usize,
    first_request: bool,
    skipped_count: usize,
}

impl Scanner {
    fn new(query: String, tx: Sender<LibraryProgress>) -> Self {
        Self {
            query: query.to_lowercase(),
            tx,
            seen_tracks: HashSet::new(),
            seen_playlists: HashSet::new(),
            scanned: 0,
            page_requests: 0,
            first_request: true,
            skipped_count: 0,
        }
    }

    async fn run(&mut self, catalog: &Catalog) -> Result<()> {
        self.scan_track_pages(catalog, Browse::Liked).await?;
        self.scan_playlist_pages(catalog).await
    }

    async fn scan_track_pages(&mut self, catalog: &Catalog, browse: Browse) -> Result<()> {
        let mut offset = 0usize;
        let mut offsets = HashSet::new();

        loop {
            if !offsets.insert(offset) {
                bail!("Saved library search stopped: catalog page offset cycle at {offset}");
            }
            self.before_page().await?;
            let page = catalog.page(&browse, offset).await.with_context(|| {
                format!("Saved library search failed at {browse:?} offset {offset}")
            })?;

            let next = page.next;
            let Rows::Tracks(tracks) = page.rows else {
                bail!("Saved library search received playlist rows while scanning {browse:?}");
            };
            self.process_tracks(tracks).await?;

            let Some(next) = next else {
                break;
            };
            validate_next_offset(offset, next, &offsets)?;
            offset = next;
        }
        Ok(())
    }

    async fn scan_playlist_pages(&mut self, catalog: &Catalog) -> Result<()> {
        let mut offset = 0usize;
        let mut offsets = HashSet::new();

        loop {
            if !offsets.insert(offset) {
                bail!("Saved library search stopped: playlist page offset cycle at {offset}");
            }
            self.before_page().await?;
            let page = catalog
                .page(&Browse::Playlists, offset)
                .await
                .with_context(|| {
                    format!(
                        "Saved library search failed while listing playlists at offset {offset}"
                    )
                })?;

            let next = page.next;
            let Rows::Playlists(playlists) = page.rows else {
                bail!("Saved library search received track rows while listing playlists");
            };
            for playlist in playlists {
                if self.seen_playlists.contains(&playlist.id) {
                    continue;
                }
                if self.seen_playlists.len() >= MAX_TRACKS {
                    bail!(
                        "Saved library search incomplete: reached the {}-playlist traversal limit",
                        MAX_TRACKS
                    );
                }
                self.seen_playlists.insert(playlist.id.clone());
                if let Err(error) = self
                    .scan_track_pages(catalog, Browse::Playlist(playlist.id.clone()))
                    .await
                {
                    let reason = match Catalog::inaccessible_playlist(&error) {
                        Some(FailureKind::AccessRestricted) => PlaylistSkipReason::Restricted,
                        Some(FailureKind::MissingItem) => PlaylistSkipReason::Missing,
                        _ => return Err(error),
                    };
                    self.skipped_count += 1;
                    self.tx
                        .send(LibraryProgress {
                            tracks: Vec::new(),
                            scanned: self.scanned,
                            complete: false,
                            skipped: vec![SkippedPlaylist {
                                id: playlist.id,
                                name: playlist
                                    .name
                                    .chars()
                                    .filter(|c| !c.is_control())
                                    .take(256)
                                    .collect(),
                                reason,
                            }],
                        })
                        .await
                        .map_err(|_| {
                            anyhow!("Saved library search cancelled: progress receiver closed")
                        })?;
                }
            }

            // Playlist-list pages do not contribute track rows, but emitting
            // their progress keeps a UI informed while a large account's
            // playlist index is being consumed.
            self.emit(Vec::new(), false).await?;

            let Some(next) = next else {
                break;
            };
            validate_next_offset(offset, next, &offsets)?;
            offset = next;
        }
        Ok(())
    }

    async fn process_tracks(&mut self, tracks: Vec<Track>) -> Result<()> {
        let mut matches = Vec::new();
        for track in tracks {
            self.scanned = self.scanned.saturating_add(1);
            if self.seen_tracks.contains(&track.id) {
                continue;
            }
            if self.seen_tracks.len() >= MAX_TRACKS {
                // Flush matches already found on this page before surfacing
                // the limit, so the caller never loses a partial delta.
                self.emit(matches, false).await?;
                bail!(
                    "Saved library search incomplete: reached the {}-track traversal limit",
                    MAX_TRACKS
                );
            }
            self.seen_tracks.insert(track.id.clone());
            if self.matches(&track) {
                matches.push(track);
            }
        }
        self.emit(matches, false).await
    }

    fn matches(&self, track: &Track) -> bool {
        self.query.is_empty()
            || track.name.to_lowercase().contains(&self.query)
            || track.artists.to_lowercase().contains(&self.query)
    }

    async fn before_page(&mut self) -> Result<()> {
        if self.page_requests >= MAX_PAGE_REQUESTS {
            bail!(
                "Saved library search incomplete: reached the {}-page traversal limit",
                MAX_PAGE_REQUESTS
            );
        }
        if !self.first_request && !PAGE_DELAY.is_zero() {
            tokio::time::sleep(PAGE_DELAY).await;
        }
        self.first_request = false;
        // Failed requests also consume the traversal budget.
        self.page_requests += 1;
        Ok(())
    }

    async fn emit(&self, tracks: Vec<Track>, complete: bool) -> Result<()> {
        self.tx
            .send(LibraryProgress {
                tracks,
                scanned: self.scanned,
                complete: complete && self.skipped_count == 0,
                skipped: Vec::new(),
            })
            .await
            .map_err(|_| anyhow!("Saved library search cancelled: progress receiver closed"))
    }
}

fn validate_next_offset(offset: usize, next: usize, visited: &HashSet<usize>) -> Result<()> {
    if next <= offset {
        bail!(
            "Saved library search stopped: catalog next offset {next} does not advance from {offset}"
        );
    }
    if visited.contains(&next) {
        bail!("Saved library search stopped: catalog page offset cycle at {next}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use serde_json::{Value, json};
    use tokio::sync::mpsc;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    const ID1: &str = "0000000000000000000001";
    const ID2: &str = "0000000000000000000002";
    const ID3: &str = "0000000000000000000003";
    const PLAYLIST: &str = "0000000000000000000011";
    const LATER: &str = "0000000000000000000012";

    async fn library_sources(server: &MockServer, later_requests: u64) {
        Mock::given(path("/me/tracks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(liked_page(vec![track(ID1, "Target liked", "Artist")], None)),
            )
            .expect(1)
            .mount(server)
            .await;
        Mock::given(path("/me/playlists"))
            .respond_with(ResponseTemplate::new(200).set_body_json(playlist_page(vec![
                json!({"id": PLAYLIST, "name":"Restricted 日本語", "owner":{"display_name":"Me"}}),
                json!({"id": LATER, "name":"Accessible", "owner":{"display_name":"Me"}}),
                // Duplicate index entries must not inflate skipped coverage.
                json!({"id": PLAYLIST, "name":"Duplicate", "owner":{"display_name":"Me"}}),
            ], None)))
            .expect(1).mount(server).await;
        Mock::given(path(format!("/playlists/{LATER}/items")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items":[{"item":track(ID2, "Target later", "Artist")}], "next":null
            })))
            .expect(later_requests)
            .mount(server)
            .await;
    }

    async fn collect_search(catalog: Catalog) -> (Result<()>, Vec<LibraryProgress>) {
        let (tx, mut rx) = mpsc::channel(16);
        let result = search(catalog, "target".into(), tx).await;
        let mut updates = Vec::new();
        while let Some(update) = rx.recv().await {
            updates.push(update);
        }
        (result, updates)
    }

    #[tokio::test]
    async fn inaccessible_playlists_keep_liked_matches_continue_later_sources_and_report_partial_coverage()
     {
        for (response, reason) in [
            (ResponseTemplate::new(403), PlaylistSkipReason::Restricted),
            (ResponseTemplate::new(404), PlaylistSkipReason::Missing),
            (
                ResponseTemplate::new(200)
                    .set_body_json(json!({"id":PLAYLIST,"type":"playlist","name":"Metadata only"})),
                PlaylistSkipReason::Restricted,
            ),
        ] {
            let (server, catalog) = catalog().await;
            library_sources(&server, 1).await;
            Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
                .respond_with(response)
                .expect(1)
                .mount(&server)
                .await;
            let (result, updates) = collect_search(catalog).await;
            result.unwrap();
            let found: Vec<_> = updates
                .iter()
                .flat_map(|u| u.tracks.iter().map(|t| t.id.as_str()))
                .collect();
            assert_eq!(found, [ID1, ID2]);
            let skipped: Vec<_> = updates.iter().flat_map(|u| &u.skipped).collect();
            assert_eq!(skipped.len(), 1);
            assert_eq!(skipped[0].id, PLAYLIST);
            assert_eq!(skipped[0].name, "Restricted 日本語");
            assert_eq!(skipped[0].reason, reason);
            assert_eq!(updates.last().unwrap().scanned, 2);
            assert!(!updates.last().unwrap().complete);
        }
    }

    #[tokio::test]
    async fn auth_throttle_server_and_invalid_responses_stop_without_skipping_sources() {
        for (response, kind) in [
            (ResponseTemplate::new(400), FailureKind::RequestRejected),
            (
                ResponseTemplate::new(401),
                FailureKind::AuthenticationRequired,
            ),
            (
                ResponseTemplate::new(429).insert_header("Retry-After", "60"),
                FailureKind::RateLimited,
            ),
            (
                ResponseTemplate::new(429)
                    .set_body_json(json!({"error":{"reason":"QUOTA_EXCEEDED"}})),
                FailureKind::QuotaExceeded,
            ),
            (ResponseTemplate::new(503), FailureKind::Server),
            (
                ResponseTemplate::new(200).set_body_json(json!({"name":"Malformed"})),
                FailureKind::InvalidResponse,
            ),
        ] {
            let (server, catalog) = catalog().await;
            library_sources(&server, 0).await;
            Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
                .respond_with(response)
                .mount(&server)
                .await;
            Mock::given(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({"access_token":"refreshed", "expires_in":3600})),
                )
                .mount(&server)
                .await;
            let (result, updates) = collect_search(catalog).await;
            let error = result.unwrap_err();
            assert!(
                crate::service::ServiceFailure::is(&error, kind),
                "{kind:?}: {error:#}"
            );
            assert!(Catalog::inaccessible_playlist(&error).is_none());
            assert_eq!(updates.iter().map(|u| u.tracks.len()).sum::<usize>(), 1);
            assert!(updates.iter().all(|u| u.skipped.is_empty() && !u.complete));
        }
    }

    #[tokio::test]
    async fn token_service_403_and_404_are_not_skippable_playlist_failures() {
        for status in [403, 404] {
            let (server, catalog) = catalog().await;
            library_sources(&server, 0).await;
            Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
                .respond_with(ResponseTemplate::new(401))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(path("/token"))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;
            let (result, updates) = collect_search(catalog).await;
            let error = result.unwrap_err();
            assert!(Catalog::inaccessible_playlist(&error).is_none());
            assert!(updates.iter().all(|u| u.skipped.is_empty()));
        }
    }

    #[tokio::test]
    async fn access_lost_on_later_playlist_page_keeps_earlier_matches() {
        let (server, catalog) = catalog().await;
        library_sources(&server, 1).await;
        Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items":[{"item":track(ID3,"Target before restriction","Artist")}], "next":"next"
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
            .and(query_param("offset", "50"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let (result, updates) = collect_search(catalog).await;
        result.unwrap();
        let found: Vec<_> = updates
            .iter()
            .flat_map(|u| u.tracks.iter().map(|t| t.id.as_str()))
            .collect();
        assert_eq!(found, [ID1, ID3, ID2]);
        assert_eq!(updates.iter().map(|u| u.skipped.len()).sum::<usize>(), 1);
        assert!(!updates.last().unwrap().complete);
    }

    #[tokio::test]
    async fn failed_requests_consume_the_bounded_traversal_budget() {
        let (server, catalog) = catalog().await;
        Mock::given(path(format!("/playlists/{PLAYLIST}/items")))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        let (tx, _rx) = mpsc::channel(2);
        let mut scanner = Scanner::new(String::new(), tx);
        scanner.page_requests = MAX_PAGE_REQUESTS - 1;
        assert!(
            scanner
                .scan_track_pages(&catalog, Browse::Playlist(PLAYLIST.into()))
                .await
                .is_err()
        );
        assert_eq!(scanner.page_requests, MAX_PAGE_REQUESTS);
        assert!(scanner.before_page().await.is_err());
    }

    async fn catalog() -> (MockServer, Catalog) {
        let server = MockServer::start().await;
        let catalog = Catalog::mock(&server.uri());
        (server, catalog)
    }

    fn track(id: &str, name: &str, artist: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "artists": [{"name": artist}],
            "type": "track",
            "duration_ms": 120000,
            "is_playable": true
        })
    }

    fn liked_page(items: Vec<Value>, next: Option<&str>) -> Value {
        json!({"items": items.into_iter().map(|track| json!({"track": track})).collect::<Vec<_>>(), "next": next})
    }

    fn playlist_page(items: Vec<Value>, next: Option<&str>) -> Value {
        json!({"items": items, "next": next})
    }

    #[tokio::test]
    async fn scans_later_liked_and_playlist_pages_dedupes_and_streams_deltas() {
        let (server, catalog) = catalog().await;
        Mock::given(method("GET"))
            .and(path("/me/tracks"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(liked_page(
                vec![track(ID1, "First Song", "Other Artist")],
                Some("next"),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me/tracks"))
            .and(query_param("offset", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(liked_page(
                vec![track(ID2, "Second Song", "Target Artist")],
                None,
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me/playlists"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(playlist_page(
                vec![json!({"id": PLAYLIST, "name": "Saved", "owner": {"display_name": "Me"}})],
                None,
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/playlists/{PLAYLIST}/items")))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "items": [{"item": track(ID2, "Second Song", "Target Artist")}, {"item": track(ID3, "Third Song", "Target Artist")}],
                "next": null
            })))
            .mount(&server)
            .await;

        let (tx, mut rx) = mpsc::channel(8);
        search(catalog, "target".into(), tx).await.unwrap();
        let mut updates = Vec::new();
        while let Some(update) = rx.recv().await {
            updates.push(update);
        }
        let found: Vec<_> = updates
            .iter()
            .flat_map(|update| update.tracks.iter().map(|track| track.id.as_str()))
            .collect();
        assert_eq!(found, vec![ID2, ID3]);
        assert_eq!(updates.last().unwrap().scanned, 4);
        assert!(updates.last().unwrap().complete);
    }

    #[tokio::test]
    async fn returns_partial_progress_and_error_after_later_page_failure() {
        let (server, catalog) = catalog().await;
        Mock::given(method("GET"))
            .and(path("/me/tracks"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(liked_page(
                vec![track(ID1, "Target Song", "Artist")],
                Some("next"),
            )))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/me/tracks"))
            .and(query_param("offset", "50"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let (tx, mut rx) = mpsc::channel(8);
        let error = search(catalog, "target".into(), tx).await.unwrap_err();
        assert!(format!("{error:#}").contains("503"));
        let mut updates = Vec::new();
        while let Some(update) = rx.recv().await {
            updates.push(update);
        }
        assert_eq!(updates[0].tracks[0].id, ID1);
        assert!(!updates.last().unwrap().complete);
    }

    #[tokio::test]
    async fn rejects_offset_that_does_not_progress() {
        // Catalog derives next from the requested offset, so a real page
        // cannot produce a cycle today.  Keep the helper assertion here to
        // lock in the guard used if that API later exposes server next URLs.
        let mut visited = HashSet::new();
        visited.insert(50);
        assert!(validate_next_offset(50, 50, &visited).is_err());
        assert!(validate_next_offset(50, 25, &visited).is_err());
        assert!(validate_next_offset(50, 100, &visited).is_ok());
    }

    #[tokio::test]
    async fn track_limit_flushes_matches_and_marks_the_result_incomplete() {
        let (tx, mut rx) = mpsc::channel(MAX_TRACKS + 2);
        let mut scanner = Scanner::new(String::new(), tx);
        let tracks = (0..=MAX_TRACKS)
            .map(|i| Track {
                id: format!("{i:022}"),
                name: format!("Track {i}"),
                artists: "Artist".into(),
                duration_ms: 1,
                playable: true,
                ..Default::default()
            })
            .collect();

        let error = scanner.process_tracks(tracks).await.unwrap_err();
        assert!(format!("{error:#}").contains("100000-track traversal limit"));
        let update = rx.recv().await.unwrap();
        assert_eq!(update.tracks.len(), MAX_TRACKS);
        assert_eq!(update.scanned, MAX_TRACKS + 1);
        assert!(!update.complete);
    }
}
