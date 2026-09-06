# Validation record

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
