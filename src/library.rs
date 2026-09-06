//! Streaming search across the account's saved Spotify library.
//!
//! The search deliberately uses the catalog's page API rather than creating a
//! task for every page.  Apart from keeping Spotify request volume predictable,
//! this means dropping/aborting the future also drops the request that is in
//! flight; there are no detached workers to finish after cancellation.

use crate::{
    catalog::{Browse, Catalog, Rows},
    model::Track,
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
/// The final successful message has an empty `tracks` vector and
/// `complete == true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryProgress {
    pub tracks: Vec<Track>,
    pub scanned: usize,
    pub complete: bool,
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
            self.page_requests += 1;

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
            self.page_requests += 1;

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
                self.scan_track_pages(catalog, Browse::Playlist(playlist.id))
                    .await?;
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
        Ok(())
    }

    async fn emit(&self, tracks: Vec<Track>, complete: bool) -> Result<()> {
        self.tx
            .send(LibraryProgress {
                tracks,
                scanned: self.scanned,
                complete,
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
