# Tuitify 0.2.9

Tuitify 0.2.9 upgrades Track Radio and Smart Shuffle with related-artist discovery,
stronger queue diversity, safer retry behavior, and improved Windows audio device
handling.

## Highlights

- **Related-artist Radio**: Track Radio uses Deezer's public related-artist metadata
  to discover artists, then searches and verifies the actual tracks in Spotify.
  Refills stay anchored to the original seed instead of drifting into suggestions.
- **More varied batches**: Radio adds up to 15 tracks per batch with at most three
  per primary artist, rejects duplicate recordings and unavailable tracks, and
  keeps unrelated Spotify search noise out of the queue.
- **Smarter Smart Shuffle**: Smart Shuffle prefers new artists around each insertion
  slot and rebalances only the unplayed portion of the queue. Played history and the
  current track remain fixed.
- **Bounded failure handling**: Similarity failures and rate limits are cached and
  surfaced instead of triggering request storms or filling the queue from a single
  artist. Radio errors stay visible until retry or a new Radio session.
- **Windows output-device refresh**: Playback can follow a changed default Windows
  output endpoint and rebuild its resampler for the new sample rate.
- **PATH installer hardening**: The installer keeps the canonical Tuitify directory
  first on the user PATH and detects another `tuitify` executable shadowing it.
- **Provider/privacy documentation**: The README and validation docs now describe
  what is sent to Deezer and how recommendation discovery differs from Spotify's
  private personalization.

## Install

Download and extract `Tuitify-0.2.9-windows-x86_64.zip`, then run:

```powershell
.\scripts\install.ps1
tuitify
```

Or download `tuitify.exe` directly. Spotify playback requires a Premium account;
`tuitify demo` requires no Spotify credentials, network, Discord, or audio device.

## Verification

- `cargo fmt --check` passed.
- `cargo test --locked`: **301 passed, 0 failed, 11 ignored**.
- `cargo clippy --all-targets --locked -- -D warnings` passed with zero warnings.
- `cargo build --release --locked` produced the optimized 0.2.9 Windows executable.
- `npm test`: **11 Microsoft Edge Playwright tests passed**.

Live recommendation and audio acceptance details from the development cycle are
documented in `docs/smart-shuffle-validation.md`.
