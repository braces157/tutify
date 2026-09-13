# Tuitify 0.2.8

Tuitify 0.2.8 introduces native album tracklist browsing, artist top tracks
discovery, dynamic breadcrumb navigation with history back-tracking, track context
menu actions, instant search resolution for Spotify web links and URIs, and
offline native demo catalog datasets.

## Highlights

- **Native Album Browsing**: Press `a` on any selected track or right-click to
  choose "View Album" to view the complete ordered tracklist with track numbers,
  durations, and playability indicators.
- **Artist Top Tracks Discovery**: Press `Shift+A` (or `A`) or choose "View Artist"
  to inspect an artist's top popular tracks and explore their catalog.
- **Breadcrumb Navigation & History Stack**: Multi-level browsing preserves your
  view stack and displays dynamic breadcrumbs (e.g. `Search › OK Computer › Thom Yorke`).
  Press `Esc` to navigate backward, seamlessly restoring your prior view, selection
  cursor, and scroll offset without re-fetching network data.
- **Instant Search URL & URI Resolution**: Paste or type Spotify Album and Artist
  web links (`open.spotify.com/album/...`, localized `/intl-.../` links) or URIs
  (`spotify:album:...`) directly into Search to instantly jump to that view.
- **Context Menu Expansion**: Right-click track context menus now include explicit
  "View Album" and "View Artist" actions across Search, Liked Songs, Queue, and
  catalog views.
- **Offline Demo Runtime Parity**: `tuitify demo` includes fictional album and
  artist datasets, allowing users to fully test album/artist browsing and
  breadcrumb navigation without Spotify credentials or internet access.
- **Contextual Play Next**: Press `p` within Album and Artist views to add the
  selected track as Play Next in the queue.

## Install

Download and extract `Tuitify-0.2.8-windows-x86_64.zip`, then run:

```powershell
.\scripts\install.ps1
tuitify
```

Or download `tuitify.exe` directly. Spotify playback requires a Premium account;
`tuitify demo` requires no Spotify credentials, network, Discord, or audio device.

## Verification

- 278 offline Rust tests passed (`cargo test --locked`); 5 environment-specific
  tests were intentionally ignored.
- Rust formatting (`cargo fmt --check`) and Clippy with warnings denied
  (`cargo clippy --all-targets --locked -- -D warnings`) passed cleanly.
- All 11 Microsoft Edge Playwright website tests passed (`npm test`).
- The release executable reported `tuitify 0.2.8`, and the native demo was
  exercised in a real terminal across album/artist views and breadcrumb navigation.

Live Spotify catalog/playback, audible output, physical media keys, LRCLIB, and
Discord presentation remain environment acceptance checks and were not exercised
for this build.
