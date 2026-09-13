# Validation record

## Second Mix Builder/application repair — 2026-09-10

Reproduced all six supplied review probes before changing behavior. Repaired
fresh-metadata pin resolution and explicit invalid-pin replacement, same-source
playlist retry pin retention, recipe-editor scroll ownership, systemic metadata
request cancellation, the 32×10 selected-preview surface, and shared native event
routing for production/demo mouse, resize, and paste events. The previously
unconfirmed demo input-parity observation was confirmed by source inspection and
synthetic `crossterm::Event` tests; real OS-generated Paste/Mouse events were not
distinguished in the PTY transport.

Checks performed on the final repair:

- `cargo fmt --check`: passed.
- `cargo test --locked`: **185 passed, 0 failed, 5 ignored**.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- `npm test`: **11 Playwright tests passed**, retaining the corrected tablet
  header and release/development install wording.
- `git diff --check`: passed with only expected LF-to-CRLF checkout notices.
- The freshly built release demo was exercised in a PTY at 80×24, 48×18, and
  32×10. A pinned suggestion survived regenerate and live resize; the minimum
  layout showed its title and pin marker; deeply scrolled details opened a visible
  recipe editor; recommendation failure recovered; replace/undo restored the
  queue; and normal exit restored the terminal. Playlist source-failure retry and
  typed 401/404/429/503/network-failure behavior were exercised with offline mocks.

These results are offline application verification. Live Spotify authorization,
catalog/audio acceptance, physical mouse paste event delivery, Discord, LRCLIB,
and production account state were not exercised or modified.

## Mix Builder and native demo repair — 2026-09-10

Added the production Mix Builder overlay/domain model, guarded playlist and
recommendation jobs, atomic local recipe persistence, structured artist IDs, and
an isolated `tuitify demo` runtime. The demo command dispatches before production
storage/authentication is opened and supplies fictional catalog, lyrics,
recommendation, and playback events through the normal App/input/UI boundaries.

The focused review follow-up repaired all twelve reported regressions: active-view
source ownership, recipe/catalog isolation, empty-apply job lifetime, demo offline
playlist actions, paused seek behavior, local demo metadata refresh, queue-source
hydration, meaningful deterministic regeneration, suggestion deduplication,
compact control/detail access, actual append counts, and explicit recipe-capacity
outcomes. Playlist Mix sources are bounded to 500 pages and 25,000 retained
candidates and are marked partial when a failure or cap leaves coverage incomplete.

Checks performed on the repaired implementation:

- `cargo fmt --check`: passed.
- `cargo test --locked`: **175 passed, 0 failed, 5 ignored**.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- `npm test`: **11 Playwright tests passed** after rebuilding the website CSS.
- `git diff --check`: passed; Git emitted only the checkout's expected
  LF-to-CRLF notices.
- Optimized Mix Builder generation test (`--release`, 45-minute target, 25%
  suggestions, artist gap 2) measured one local sample of **0.34 ms / 2.25 ms /
  14.61 ms / 67.90 ms** for 500 / 5,000 / 25,000 / 100,000 candidates,
  producing 10 entries at each size. This is a domain microbenchmark, not UI
  latency or a performance SLA.
- Launched the newly built `target\release\tuitify.exe demo` in PTYs at 80×24,
  48×18, and 32×10. Verified playlist → Queue → Mix uses the queue; the first
  recommendation request fails visibly and `g` recovers; `?` exposes the actual
  seed/provider; pin/regenerate/save/replace/undo work; a recipe can reopen and
  cancel from Liked Songs; F5 restores fixture metadata locally; fictional
  playlist enqueue adds 23 playable tracks; a paused seek remains paused; and
  all sessions quit with alternate-screen/terminal restoration.

The automated, benchmark, and PTY checks are offline verification. Live Spotify catalog
paging/recommendations, real account-specific availability, librespot playback,
WASAPI audio, Discord, and Lrclib were not exercised for this milestone. The four
ignored tests remain the documented live-streaming, Windows media-session,
terminal-cleanup, release-render benchmark, and release Mix Builder benchmark
opt-ins.

## Structural refactor — 2026-09-08

Refactored the current working tree while preserving the existing feature work.
Application input/actions, controls, background jobs, persistence, browsing, lyrics
state, and runtime now have focused modules. UI panels, terminal lifecycle, themes,
navigation, and frame decoration are separated. `app.rs` decreased from 4,054 to
589 lines and `ui.rs` from 2,560 to 151; App has 24 top-level fields instead of 57.
All Rust files are below 1,000 lines, including tests; the largest is 839 lines.

Behavioral changes enforce the reviewed boundaries: stats selection is independent
of catalog selection, overlays use a mutually exclusive enum, context menus invoke
semantic actions, playback controls share one implementation, drawing receives
explicit mutable layout feedback, and authentication banners follow typed catalog
health rather than status-message substrings. Queue JSON, statistics JSON, cache,
configuration and credential formats are unchanged. See `ARCHITECTURE.md` for the
resulting module map and invariants.

Checks performed on this implementation:

- `cargo test --locked --quiet`: **142 passed, 0 failed, 4 ignored**. A test-name
  comparison with the pre-refactor working-tree backup confirmed all 141 original
  tests (including the four ignored tests) remain; five regression tests were added.
- New coverage checks equivalent seek/volume/mute commands in normal and stats
  modes, semantic menu actions and modal isolation, catalog selection through
  stats navigation with actual rendered frames, typed authentication health and
  recovery across catalog clones, and banner independence from status wording.
- `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, and
  `git diff --check`: passed.
- `cargo build --release --locked`: passed. The release executable passed
  `--version` (0.2.5) and `--help` smoke checks.
- The opt-in `ui::tests::terminal_cleanup_acceptance` test passed separately in a
  PTY: raw mode restored after normal exit and an intentional caught panic.
- The opt-in `media_controls::tests::windows_media_session_acceptance` test passed
  separately: silent Windows session metadata, controls, timeline, and cleanup.

The two acceptance tests were invoked directly on the just-built debug test
executable using `--exact`, `--ignored`, `--nocapture`, and `--test-threads=1`.
Live Spotify streaming and the optimized rendering benchmark were not rerun.
Website files were not changed and browser tests were not rerun. No new runtime
performance claim is made.

Installation follow-up: rebuilt the release executable and replaced
`%USERPROFILE%\.cargo\bin\tuitify.exe`. Its SHA256 matched the release build.
The bare `tuitify` command resolved through PATH, reported version 0.2.5, opened
the TUI in a PTY, restored the saved queue paused, and exited cleanly with `q`.
Catalog access reported an expired Spotify login; live music access requires
`tuitify auth --force`. This launch does not establish successful streaming.

## Earlier validation records

Date: 2026-09-05. Platform: Windows x86_64, Rust 1.95.0, Windows audio via
Rodio/CPAL (WASAPI), librespot 0.8.0. This file records acceptance evidence without
account identifiers, tokens, library contents, or listening logs.

## Live checks performed

- Personal Developer app PKCE login completed; tokens saved in Windows Credential
  Manager. A single app token connected to the Spotify session but failed audio
  metadata retrieval. A separate librespot PKCE streaming login fixed playback.
- The catalog flow now follows Spotatui's shared PKCE client and `/login` redirect
  by default; `--client-id` keeps the personal-app `/callback` flow available.
- Streaming login verifies the account after the librespot handshake, when the
  canonical username is populated, instead of querying the streaming token through
  the Web API.
- Spotify desktop processes were closed before the successful playback probe.
  The user explicitly confirmed that audio was audible through Windows output.
- Probe pause/resume and ten-second seek produced the corresponding player
  events. Volume commands were exercised.
- TUI playback controls now expose one-percent volume steps, mute/restore,
  start/end seeking, and a progress bar with elapsed, total, percentage, and
  remaining time.
- TUI search returned ten results; playing a search result worked.
- Liked songs and playlists loaded. Playing from liked songs and an owned
  playlist worked through the same streaming worker.
- Appending, removing, shuffle, repeat, volume, pause, and normal terminal exit
  were exercised in the TUI.
- The final release executable restored 48 queue entries, selection, volume,
  shuffle, repeat, and the saved position paused. The position remained unchanged
  while idle. A second player process was correctly rejected by the instance lock.
- Opt-in live integration test passed: start, pause, seek while paused, resume,
  volume acknowledgement, deliberately invalidated streaming session, reconnect
  without another browser login, completion after seeking near the real end of
  the track, and credential reuse by a new player worker.
- The real-terminal cleanup test passed after normal exit and an intentional
  caught panic, including verification that raw input mode was disabled afterward.

## Automated coverage

Offline tests cover queue order and duplicate occurrences, current-track-preserving
shuffle and unshuffle, repeat behavior and manual next, previous/restart, invalid
snapshots, stale completion events, unavailable-track stopping, atomic replacement,
corrupt-file preservation, version rejection, leftover temporary writes, instance
locking, OAuth state/PKCE, refresh serialization and revocation, rate limits,
Retry-After seconds and HTTP dates, ten-result search pagination, playlist `/items`
response parsing, metadata-only restrictions, 403/503 errors, 401 refresh, URL
validation, and all views at 120x35, 80x24, 48x18, 32x10, and 20x6.

Final results: 31 offline tests passed; formatting and Clippy with warnings denied
passed; the optimized Windows release build succeeded. Both opt-in acceptance
tests (streaming and terminal cleanup) passed separately. Run the commands in
README.md to reproduce checks. These two tests are ignored by default because
they require live credentials/audio or a real terminal.

## Practical limits

Login follow-up: five additional mocked tests cover the final callback result,
long and short verification rate limits, bounded retries, and denied app access.
The callback no longer reports receipt before the eventual setup failure is known.
These checks do not claim Spotify's current live quota has cleared.

Guided setup follow-up: three additional tests cover missing-login selection,
client changes, explicit forced login, and reuse of saved refresh tokens even
when access tokens have expired. Startup decisions read local credentials only;
the guided browser flow was not replayed against the rate-limited live account.

- Session-loss recovery was tested by shutting down the player's Spotify session,
  not by disabling the user's network adapter. A real Wi-Fi/router outage remains
  a manual environment-specific acceptance check.
- Missing audio devices and corrupted storage have fallible error paths; no
  physical output device was unplugged during acceptance.
- Normal exit and an unwinding panic restored the terminal; forced process
  termination cannot run the restoration hook.
- Rendering tests use Ratatui's test backend. Real terminal font availability,
  especially CJK glyph fallback, can vary.
- Windows executable is a personal, unsigned release build. No installer,
  code-signing certificate, updater, or background service is included.


## Performance/reliability follow-up — 2026-09-06

The review in PERFORMANCE_REVIEW.md was followed by fixes for visible-row
rendering and cached filters, quiet paused rendering, bounded streaming metadata
hydration, explicit metadata errors/retry, independent changed-state writers,
cache expiry/capacity/account cleanup, J dispatch, lyrics readiness/retry/parsing,
queue-generation checks, full playlist pagination, cancellable streaming
connection setup, and wrapping-aware Help/plain-lyrics scrolling.

This follow-up's offline Rust run passed 67 tests with 3 ignored (the two original
live/terminal acceptance tests and the new manual release rendering benchmark).
Formatting and Clippy with warnings denied passed. Additional tests exercise
503 metadata errors, early streaming results, delayed playlist failures, stale
queue/lyrics responses, save failure/recovery, no-op checkpoints, delayed
connection controls/cancellation, queue capacity, cache cleanup, and the last
Help/plain-lyrics line at 32×10.

The optimized Windows executable rebuilt successfully and passed `--help` and
`--version` smoke checks. `cargo fmt --check` and `git diff --check` passed.
`npm ci` and all 11 Microsoft Edge browser tests passed; `npm audit` reported
zero vulnerabilities with Playwright pinned to 1.58.2. Browser coverage includes
local-only assets, distinct views, idempotent radio counts, track-specific lyrics,
keyboard focus retention, scoped shortcuts, arrow-key tabs, native range controls,
all five theme accents, paused/offscreen/reduced-motion animation, mobile width,
and clipboard rejection or missing API. Desktop (1440 px) and mobile (390 px)
screenshots were inspected; mobile controls now wrap and mini-bars have explicit
height. These checks do not establish Core Web Vitals or whole-browser CPU usage.
The local preview server also passed HTTP smoke checks for root/canonical pages,
root-relative assets, malformed URI (400), path traversal (403), and missing
files (404), remaining healthy throughout. Serving is restricted to `docs/`.

The optimized TestBackend benchmark measured a 5,000-entry queue frame at
0.161 ms and a cached filtered-catalog frame at 0.204 ms. See BENCHMARKS.md for
all sizes, the earlier review baseline, methodology differences, reproduction,
and limits. These results do not measure total process CPU, RAM, startup, or
real terminal output.

The live Spotify/audio and real-terminal acceptance tests were not rerun in this
follow-up. Earlier live results above describe the earlier build only. Current
live-service compatibility and physical device behavior remain environment
acceptance checks; no new claims are made about those measurements.

## Version 0.2.1 release acceptance — 2026-09-06

The 0.2.1 release repeated the 67 offline tests, formatting, Clippy, optimized
build, 11 Edge tests, and npm audit successfully. The installed PATH executable
was replaced with the release build and its SHA256 matched. Invoking `tuitify`
opened the TUI; `tuitify --version` returned 0.2.1. The live launch exposed an
expired Spotify login; both browser authorization steps succeeded, and the
installed app then loaded Liked Songs. Explicit reauthorization resets the queue
under the existing account-isolation policy.

The opt-in real-terminal cleanup test passed normal exit and caught-panic
restoration. The opt-in live streaming test initially encountered a two-second
Spotify rate limit; after waiting, it passed audio-start events, pause, seek,
resume, volume acknowledgment, injected session loss/reconnection, completion,
and credential reuse. These checks verify playback-engine events and terminal
behavior on this Windows setup, not a subjective listening-quality assessment.

## Version 0.2.2 mouse controls — 2026-09-06

Mouse support adds row/view selection, right-click action menus, wheel navigation,
click-to-edit search/filter, playback badge toggling, and progress-bar seeking.
The menu supports keyboard navigation and rejects actions after its underlying
list changes. Hit regions are generated only for rendered rows and reset when
the layout/data changes; mouse movement alone does not schedule a redraw.

Validation: 71 offline Rust tests passed (four new mouse regressions), along with
formatting, Clippy, optimized build, and 11 Edge tests. Regressions cover filtered
track actions, scrolled queue indices, stale menus, queue-sidebar selection,
menu bounds at 32x10, wheel navigation, and playback-badge dispatch. A live PTY
launch of the installed executable accepted an SGR right-click and displayed the
queue-row action menu; Esc dismissed it and q exited. The real-terminal cleanup
test exposed Windows mouse capture restoring raw mode, which was fixed by
releasing mouse capture before disabling raw mode. The test then passed both
normal exit and caught-panic restoration. Live audio was not rerun for this
mouse-only update; the 0.2.1 live results above describe the preceding build.

## Windows media controls — 2026-09-07

Added a Windows System Media Transport Controls session backed by a metadata-only
MediaPlayer. Librespot remains responsible for audio. Media actions share the
terminal's playback/queue logic and bypass search/filter text entry. The session
publishes title, artist, playback state, and timeline; it disables itself for an
empty queue and unregisters callbacks/closes on exit. WinRT work runs on a
dedicated thread with bounded snapshot delivery.

Validation: 92 offline Rust tests passed, including new command-idempotence,
search-entry isolation, manual next/repeat, previous/restart, and bounded update
tests. Clippy with warnings denied, formatting, diff checks, and the release build
passed; the release executable passed its version smoke check.
The opt-in silent Windows acceptance test passed on this desktop: it
discovered the session through the Windows global media-session API, checked
title/artist/position/state, delivered Play/Pause/Next/Previous through Windows,
observed state updates, and verified session removal after cleanup. The test
does not stream audio or exercise physical keyboard/headset hardware. Live
Spotify playback and manual hardware-key routing with competing players were
not rerun for this change.

## Version 0.2.4 release review — 2026-09-07

Reviewed Discord IPC framing, response matching, reconnect timing, shutdown,
activity disclosure, metadata compatibility, Windows media controls, and release
version/download references. Fixed unbounded IPC reads and allocations; added
READY/nonce/error validation and ping replies; replaced buffered activity updates
with a latest-state watch channel; corrected stale reconnect timestamps and
same-track metadata updates; bounded RPC frequency, artwork response size, and
shutdown. Restored queues and unloaded, unknown, loading, or failed tracks do not
publish listening activity. Album fields remain optional in older metadata caches.

Removed the normal test that could publish a synthetic activity to real Discord.
All Discord protocol tests use private in-memory transports or an isolated
Windows named pipe. Coverage includes interrupted connections, reconnect without
a track change, activity clearing, ping/pong, unrelated responses, rejected
commands, oversized packets, timeouts, coalescing, Unicode field limits, and
forced cancellation of stalled shutdown. No Discord credentials are used.

Release checks: 106 offline Rust tests passed (four opt-in tests excluded),
Clippy with warnings denied and formatting passed, and all 11 Edge browser tests
passed after npm ci. npm audit reported zero vulnerabilities. The silent Windows
media-session acceptance test passed again for v0.2.4, including metadata, state,
timeline, all four media commands, and cleanup.

Live Spotify/audio playback, physical media keys, and the actual Discord profile
appearance/application registration were not exercised for this release. Mock
IPC acceptance does not establish Discord server-side presentation or policy.

## Version 0.2.5 release review — 2026-09-07

Reviewed local aggregate song statistics implementation, accounting semantics,
overlay presentation, persistence lifecycle, and privacy boundaries. Statistics
track play count and cumulative listened time per track in `%LOCALAPPDATA%\Tuitify\stats.json`,
capped at 50,000 entries. Wall-clock listened time is clamped to 5-second accounting
intervals during active playback, preventing false increments during pause or seek.
A play is credited once per track generation when listened time reaches 30 seconds
or 50% of track duration (whichever is shorter), or on track completion. Track entries
retain fallback title and artist names to preserve display usability even if metadata
cache entries expire.

Privacy and lifecycle semantics: all statistics remain completely local and offline.
No timestamped listening logs, history, or telemetry are recorded or transmitted.
Corrupted or invalid versions of `stats.json` are safely rejected. Re-login to the
same verified account preserves statistics, queue, and cache; switching accounts or
explicit logout (`tuitify logout`) purges `stats.json`. `tuitify clear-cache` purges
the metadata cache without clearing statistics. The `Shift+S` overlay displays
sorted tracks by play count and listening time, dismissible with `Esc` without affecting
playback.

Checks executed: 130 offline Rust tests passed (four opt-in tests excluded: live streaming,
silent Windows media session, terminal cleanup, and render scaling benchmark),
`cargo fmt -- --check` and `cargo clippy -- -D warnings` passed cleanly, and `cargo check --locked`
succeeded. Web validation passed with all 11 Edge browser tests passing and npm audit reporting
zero vulnerabilities.

Live Spotify audio streaming, real-time hardware key interactions, and long-term
multi-week storage accumulation were not exercised for this release.

## Version 0.2.6 release milestone — 2026-09-10

Reviewed and built the Smart Shuffle, Mix Builder, and credential-free native
demo milestone. Mix generation is deterministic, preserves pinned occurrence
identity, uses structured artist IDs when available, reports achieved duration
and suggestion share, and labels partial or source-only results. Preview work is
separate from queue mutation; replace and append use the existing bounded undo
history and reject stale background responses. Demo startup is dispatched before
production storage, authentication, Discord, or playback services are opened.

Checks executed on Windows: `cargo fmt --check`, `cargo test --locked` (185
passed, 5 opt-in tests ignored), `cargo clippy --all-targets --locked -- -D
warnings`, and `cargo build --release --locked` all passed. The freshly built
`target\release\tuitify.exe` reported version 0.2.6 and exposed the `demo`
subcommand. All 11 Edge Playwright website tests passed. `git diff --check`
reported no whitespace errors (only the repository's expected LF-to-CRLF working
tree notices).

The release executable was also exercised in a real PTY: the native demo opened,
Mix Builder displayed the reproducible recommendation outage, retry recovered
suggestions, a preview position remained pinned, replacing the queue succeeded,
undo restored the original nine-track queue, normal quit restored the terminal,
and no production credentials, storage, network, Discord, or audio device were
used. Earlier milestone acceptance additionally covered 80x24, 48x18, and 32x10
layouts before the version-only rebuild.

Live Spotify catalog/playback, audible output, physical media keys, LRCLIB,
Discord presentation, and playlist access against a real account were not tested
for this release milestone and remain live acceptance work. The v0.2.6 release
package retains those limitations in its notes; website install commands and
download links are pinned to the matching v0.2.6 tag and assets.

## Version 0.2.7 release milestone — 2026-09-12

Reviewed and built the semantic TUI redesign and the accompanying queue,
authentication, Unicode-input, Mix Builder, installer, documentation, and
website updates. Every theme now owns a complete palette; focus, selection,
current playback, and application state remain distinguishable when combined.
Responsive rendering coverage includes 120x35, 80x24, 48x18, 32x10, and 20x6
across all five themes and paused, loading, playing, and failed states.

Checks executed on Windows: `cargo fmt --check`, `cargo test --locked` (193
passed, 5 opt-in tests ignored), `cargo clippy --all-targets --locked -- -D
warnings`, and `cargo build --release --locked` all passed. The optimized
`target\release\tuitify.exe` reported version 0.2.7, displayed help, opened the
native demo in a real PTY, and restored the terminal normally on quit.

Website validation ran after `npm ci`: npm audit reported zero vulnerabilities,
the production Tailwind CSS rebuilt successfully, and all 11 Microsoft Edge
Playwright tests passed. The browser demo theme order and labels now match the
Rust application. Release packaging produced the versioned Windows ZIP, raw
executable, SHA-256 checksum files, release notes, and the idempotent per-user
PATH installer under `scripts/install.ps1`.

Live Spotify catalog/playback, audible output, physical media keys, LRCLIB, and
Discord presentation were not exercised for this release and remain
environment-specific acceptance checks.

## Version 0.2.8 release milestone — 2026-09-13

Reviewed and built the native Album and Artist browsing milestone with dynamic
breadcrumb navigation, view stack history, track context menu actions, universal
Spotify URI/URL search resolution, and offline demo catalog datasets. The navigation
stack preserves view, browse, rows, selected cursor, scroll offset, and breadcrumb
trail, popping cleanly with Esc without issuing redundant network requests. The
search parser accepts standard URIs, web URLs, internationalized locale paths,
and query parameters for tracks, albums, and artists. Track context menus expose
"View Album" and "View Artist" on all individual track rows while appropriately
excluding them from playlist directory rows.

Checks executed on Windows:

- `cargo fmt --check`: passed with zero formatting diffs.
- `cargo test --locked`: **278 passed, 0 failed, 5 ignored**. Coverage expanded
  from 193 to 278 tests, including navigation stack transitions, breadcrumb trail
  formatting and adversarial width fuzzing, context menu hit-testing and revision
  guards, universal URI/URL parsing, and offline demo catalog parity.
- `cargo clippy --all-targets --locked -- -D warnings`: passed cleanly with zero warnings.
- `cargo build --release --locked`: passed, producing optimized `target\release\tuitify.exe`.
- `npm test`: **11 Microsoft Edge Playwright tests passed** with zero vulnerabilities.
- Native demo exercised in real PTY: tested `tuitify demo` with album tracklist
  inspection (`a`), artist top tracks discovery (`Shift+A`), breadcrumb header
  rendering, and `Esc` back-navigation without Spotify credentials or network access.

Live Spotify streaming audio, physical media keys, and live account authentication
remain environment-specific acceptance checks and were not exercised for this build.
