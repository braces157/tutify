# Changelog

## 0.5.0 — 2026-10-07

- Make `tuitify` the complete launch and setup flow. Connect Spotify or a Google
  library inside F6 Tools; flush saved state before browser sign-in and return to
  the player afterward. Automatically open free music when account checks fail
  or subscription details are unavailable, retaining the visible cause.
- Separate account/source policy, session lifecycle, provider services and TUI
  runtime. Cache successful account-scoped plan checks for ten minutes, bound
  lookups to three seconds, and route confirmed Premium expiry to free music.
- Bound parsed catalog pages by a 32 MiB LRU budget, preserve remote offsets for
  invalid rows, and release the idle Python metadata helper after 30 seconds
  without interrupting audio or discarding cached pages. Use two Tokio workers
  and publish desktop metadata on changes instead of cloning it every frame.
- Add repeatable Windows ConPTY speed, CPU and process-tree memory benchmarks;
  validate exact queries and visible result rows, preserve user state, and report
  upstream-blocked playback separately from successful playback measurements.

- Select Spotify automatically for saved Premium accounts, and YouTube Music for
  free accounts or users without a Spotify login. Keep explicit `--source spotify`,
  `--source youtube`, and `--source auto` choices. Prepare missing YouTube tools and
  music search/radio on first launch, keep provider queues separate, and skip the
  Spotify streaming login for free accounts. Preserve account-check errors instead
  of treating expired credentials, denied access or rate limits as a free plan.
  Subscription checks that omit details now use the existing streaming login
  where available, then fall back to free music without requiring a source choice.

- Reuse the YouTube Music helper process and HTTP session, returning
  the first search batch without extra continuation requests, and keeping cached
  library views responsive during network work. Prepare the next audio stream
  while playback continues and reuse a bounded, expiring stream cache for skips
  and repeats; cancellation stops owned helpers and obsolete preparation.

- Add optional Google library connection through a dedicated Chrome sign-in
  window (Edge fallback), user-encrypted session storage and an isolated,
  hash-pinned YouTube Music adapter. Browse playlists and Liked Songs, search
  the saved library, open public playlist links and music album/artist views,
  and load complete lists into the existing queue and Smart Shuffle algorithms.
  Preserve song metadata when playback resolves generic video metadata; expose
  read-only playlist previews and logout commands.

- Add optional YouTube playback without Spotify authentication: `tuitify youtube`
  or `--source youtube`, song/artist search, video links, a separate persisted
  queue, transport controls, PCM visualizer, media keys and lyric lookup.
  Resolve streams with yt-dlp/Deno and decode bounded PCM with FFmpeg; cancel
  obsolete work when seeking, skipping or stopping. Add checksum-verified tool
  setup, tool diagnostics and a muted real-audio probe. Spotify remains the
  explicit Premium source and keeps its existing account data.

## 0.4.0 — 2026-10-02

- Future plan 110/112: embed source, commit/dirty status, compiler/target/settings
  and build IDs in detailed version, doctor and redacted support output. Generate
  per-file and external artifact manifests, reject stale builds, package from a
  fresh bounded workspace and verify extracted payload hashes and local links.

- Future plan 101: raise the declared Rust minimum to 1.88 because the application
  and locked `wiremock` tests use let chains. Add a separate Windows x64 CI job
  that reads the minimum from the manifest, tests the locked graph and builds the
  release; keep stable formatting and strict Clippy independent.

- Future plan 010: keep up to 64 classified session errors independently of the
  status line and add an F7 panel with a redacted JSON report preview. Export
  exactly the reviewed snapshot to a new external file; never overwrite files,
  save the journal with listening data, or upload reports automatically. Add an
  offline `support` command for startup diagnostics and summarize per-session
  catalog observations without resource IDs. Exclude secrets, raw payloads,
  personal paths, device names, searches and song history by construction.

- Future plan 009: add offline-by-default `doctor` diagnostics for exact build
  hashes and launch paths, terminal/data/state health, saved credential expiry
  and account mapping, and output devices. Explicit network probes are bounded
  GETs with no login, refresh, streaming handshake, discovery fallback or writes.
  Preserve interrupted restore journals, report scoped capabilities and actionable
  failures, and offer structured local JSON output.

- Future plan 008: add `state inspect`, component backup, and previewed per-file
  reset/restore commands. Report file/version/invariant failures without raw
  values, preserve damaged/future snapshots in exact-byte component backups,
  recover recipes without touching queues or credentials, and keep unsupported
  formats intact during ordinary writes and whole-state restore. Archive originals
  before confirmed changes and reuse journal recovery for single files and cache.

- Future plan 007: separate stable Web API account identity, legacy user IDs,
  and streaming usernames in versioned credential metadata. Verify account
  mappings, preserve queue/cache/statistics through same-account alias changes,
  reject mismatched or ambiguous replacements without clearing saved files, and
  retain metadata during token refresh. Check the authenticated playback username
  before audio starts; restore prior credential metadata after failed writes.

- Future plan 006: add versioned, credential-free saved-state backups and a
  validated restore preview with confirmation bound to the backup, destination,
  and current files. Preserve duplicate queue occurrences, recipe settings, and
  existing aggregates; exclude cache and runtime state. Stage replacements before
  publishing and recover interrupted restores under the instance lock.

- Future plan 004: label artist results by their actual source: Top Tracks,
  verified Artist Search, or fictional Demo Tracks. Keep Artist Search pagination
  on its established source, distinguish empty/error states, retain provenance in
  navigation history, and recheck Top Tracks on F5 without relabeling retained rows.

- Future plan 003: continue saved-library searches past inaccessible playlists,
  retaining earlier matches and each skipped source's name and reason. Report
  partial coverage even when traversal finishes; F4 opens a scrollable source
  list and F5 rechecks access. Keep authentication, quota, outage, and malformed
  responses fatal; preserve skipped sources through cancellation and navigation.

- Future plan 002/005: remember catalog access results within the current
  client/account session, with bounded expiry and per-item scopes. Avoid
  repeated denied requests; F5 and Mix Retry recheck access without bypassing
  service waits. Distinguish playlist metadata-only access from malformed
  responses, keep token-refresh quota failures visible, and preserve fallback
  errors. Empty successful recommendation pools remain empty Spotify results.

- Future plan 001: classify catalog/login/provider failures and distinguish
  exhausted Spotify quota from temporary rate limits. Preserve retry deadlines
  and safe failure causes across cooldowns and cached similar-artist lookups.
  Keep authentication/quota/outage errors visible through discovery fallbacks,
  bound error-body parsing, and omit upstream URLs/payloads from diagnostics.

- Add an F6 Listening Tools menu with 15/30/45/60-minute sleep timers,
  stop-after-current-track, cancellation, and a visible countdown. Expiry keeps
  the queue and pauses playback; end-track mode overrides repeat and cancels
  when playback switches tracks.
- Add undoable full-queue cleanup of played entries, duplicate upcoming track
  IDs, and known unavailable tracks, preserving the current occurrence and
  playback position. Bulk edits remap shuffle/suggestion indices in one pass.
- Add Ctrl+Enter Play Next across track views. Queue promotes the existing
  occurrence without copying it. Recall 20 recent searches with Up/Down during
  search entry and recover the last cleared queue filter with Ctrl+R.

- Add instant Queue filtering by title, artist, and album with Unicode case
  matching, match counts, original queue positions, and clickable edit/clear
  controls that fit a 32 × 10 terminal. Matches update as metadata loads.
- Keep filtered playback, reorder, remove, context menus, and undo tied to the
  correct queue occurrence, including duplicates and shuffled queues. Prevent
  hidden-track actions on empty results and require clearing a filter before
  clearing the entire queue. `.` clears the filter and reveals the current track.

## 0.3.1 — 2026-09-30

- Fix Vietnamese Radio lookup when catalogs differ in Latin accents. Confirm
  aliases with the seed recording and retain exact-name/artist-ID safeguards.
- Fix duet lyrics by retrying the primary artist after a missing full-credit
  result, validating the returned title, artist, and duration. Preserve Unicode
  accents and normalize canonically equivalent character sequences.
- Retry temporary lyrics HTTP 502–504 failures once with a short delay, honor
  longer cooldowns, and show clear server/rate-limit errors with F5 retry.
- Complete Statistics and Mix Builder mouse controls, including wrapped controls,
  long searches, long Unicode recipe names, and scrollable details.
- Preserve invalid statistics/recipe files by stopping startup before workers run.
- Buffer atomic JSON saves and share immutable snapshots instead of cloning
  collections again in writer tasks. Refresh statistics with one normalization
  and sort pass and update hydrated metadata per track.
- Correct Glass refresh deadlines and use the actual output-device sample rate
  for FFT frequency bands.
- Restrict artist-catalog fallback to HTTP 403 and validate Spotify artist IDs.
- Refresh the website, guides, validation record, benchmark procedures, and
  release downloads. Include linked documentation and the installer in the ZIP.

## 0.3.0 — 2026-09-15

- Introduce the Glass theme, custom wallpaper backgrounds, portable Unicode
  rendering, and a dedicated native-resolution Windows Terminal profile.
- Add `background`, `--glass`, and `--glass-window` controls and readable queue,
  lyrics, and visualizer surfaces over detailed artwork.

Earlier release history is available in [GitHub Releases](https://github.com/braces157/tutify/releases).
