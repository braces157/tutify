# Monochrome playback validation — 2026-10-23

Automatic playback uses **Monochrome → Volta → Spotify → YouTube**, skipping
services without the required local connection or tools. Explicit source modes
retain their selected behavior.

Monochrome searches `https://tracks.monochrome.st/search/tracks` by artist and
title, matches the original recording's ISRC when available, and streams
`/track/{trackId}`. Without an ISRC, matching requires an unambiguous title,
artist and compatible duration. The original queue identity is preserved.
The first 42 audio bytes supply the FLAC bit depth and sample rate; FFmpeg then
decodes the stream through the existing Windows output path. No shared API key,
browser token or origin impersonation is used.

A live, muted production-provider probe of **Be a flower — Ryokuoushoku Shakai**
(`deezer:2493747201`) played at least five seconds through Monochrome, reporting
**FLAC 24-bit / 48 kHz** and retaining the original Deezer identity. This is a
short playback check, not a full-song continuity test or a claim about Windows
output being bit-perfect. The separate [full-track YouTube validation](be-a-flower-playback-validation.md)
records the earlier continuity investigation.

Regression checks cover the four-provider order, missing recordings, rejected
alternate versions, malformed IDs, HTTP 429 cooldowns, closed workers, pause,
seek, track changes, stale events, original metadata and measured FLAC quality.
Rate-limited requests respect the server's cooldown instead of repeatedly
requesting audio.

To repeat the short automatic-path probe with muted output:

```powershell
cargo test --locked live_automatic_audio_probe -- --ignored --nocapture
```

The probe does not write saved queues or listening statistics. Live provider
availability may change, so a later run can exercise a fallback instead.
