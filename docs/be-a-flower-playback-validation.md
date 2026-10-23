# Be a flower playback validation — 2026-10-22

> **Dated record.** One recording, one machine, muted output. It documents a
> specific continuity investigation and its fix; it is not a general playback
> guarantee. See [Monochrome playback validation](monochrome-playback-validation.md)
> for the automatic-provider probe and the [documentation index](README.md).

Tested the studio recording **Be a flower — Ryokuoushoku Shakai**, discovered as
`deezer:2493747201` and matched to the official YouTube recording
`youtube:voAu0YRWUNE`. The catalog lists 200 seconds; decoded audio lasts about
199.33 seconds. Tests used muted Windows audio, real providers, and next-song
preloading. Saved queues and listening statistics were not changed.

| Playback path | Observed result |
| --- | --- |
| Spotify + Volta, before the transport change | FLAC 24-bit / 48 kHz played through 199,333 ms with no recovery. |
| Deezer discovery to YouTube, before the title change | Failed before playback: the bilingual title `花になって - Be a flower` was rejected. |
| Direct YouTube, before the transport change | Premature completion at 45,393 ms, 94,194 ms, and 130,274 ms. The first two resumed; the third exhausted the two song recovery attempts. |
| Deezer discovery to YouTube, after both changes | Opus 127 kbps / 48 kHz played through 199,333 ms, completed naturally, and needed zero song recovery attempts. Total run time including resolution was 205.07 seconds. |

The premature completions arrived as successful decoder EOF events rather than
track errors. A read-only stream-header check advertised the complete 3,168,765
byte file and byte-range support. The fix enables FFmpeg reconnection for an
interrupted finite HTTP response, with a five-second maximum reconnect delay.
It does not enable reconnecting at a natural EOF. See the
[FFmpeg HTTP protocol documentation](https://ffmpeg.org/ffmpeg-protocols.html#http).

Bilingual title aliases now match only complete title components across scripts,
while artist and duration checks remain required. Live, instrumental, cover,
remix, and other version annotations cannot be discarded to force a match.

A generated FLAC regression test deliberately closes an HTTP connection halfway
through its advertised body. It verifies that the second request resumes at the
exact byte offset, delivers all 288,000 channel samples (144,000 stereo frames)
once, and terminates at
the natural end.

Validation passed: 696 application tests, 7 build-identity tests, bilingual title
regressions, the real interrupted HTTP test, full-track live playback, formatting,
Clippy with warnings denied, and the locked release build. Optional live tests
remain ignored during the normal suite.

To repeat the complete production-path diagnostic in PowerShell:

```powershell
$env:TUITIFY_LIVE_TRACE = '1'
$env:TUITIFY_LIVE_SOURCE = 'youtube'
$env:TUITIFY_LIVE_PRELOAD = 'deezer:2493755251'
cargo test --locked live_complete_track_diagnostic -- --ignored --nocapture
```

Signed audio URLs, cookies, and credentials are omitted from this record.
