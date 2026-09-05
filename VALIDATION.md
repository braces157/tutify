# Validation record

Date: 2026-09-05. Platform: Windows x86_64, Rust 1.95.0, Windows audio via
Rodio/CPAL (WASAPI), librespot 0.8.0. This file records acceptance evidence without
account identifiers, tokens, library contents, or listening logs.

## Live checks performed

- Personal Developer app PKCE login completed; tokens saved in Windows Credential
  Manager. A single app token connected to the Spotify session but failed audio
  metadata retrieval. A separate librespot PKCE streaming login fixed playback.
- Spotify desktop processes were closed before the successful playback probe.
  The user explicitly confirmed that audio was audible through Windows output.
- Probe pause/resume and ten-second seek produced the corresponding player
  events. Volume commands were exercised.
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

Final results: 29 offline tests passed; formatting and Clippy with warnings denied
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
