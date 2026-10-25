# Monochrome playback validation — 2026-10-23

> **Dated record.** Short live probes on two recordings, plus the automated
> regressions listed below. It is not a sustained-playback or device-matrix
> result. For current user-facing behavior see
> [How automatic audio works](../README.md#how-automatic-audio-works).

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
seek, track changes, stale events, optional-tool startup failures, initial source
labels, original metadata and measured FLAC quality. Natural completions allow
the recording match's six-second duration drift plus codec padding.
Rate-limited requests respect the server's cooldown instead of repeatedly
requesting audio.

To repeat the short automatic-path probe with muted output:

```powershell
cargo test --locked live_automatic_audio_probe -- --ignored --nocapture
```

The probe does not write saved queues or listening statistics. Live provider
availability may change, so a later run can exercise a fallback instead.

## Panorama browser request comparison and connection fix

An additional investigation on 2026-10-23 compared **Panorama — IZ*ONE**
(`deezer:1746860357`, ISRC `KRA492001345`) with the website's actual playback.
Both use `https://tracks.monochrome.st/track/153941088392974336`.
The captured successful Chrome request used `Range: bytes=0-` and HTTP/3 over
IPv6, with no cookies or Authorization header. Earlier browser requests failed
before a subsequent request returned HTTP 206 and real audio. Copying the
browser's visible headers into a native request did not fix the failure.

Controlled requests with Tuitify's own user agent and `Range: bytes=0-41`
returned HTTP 429 over IPv4 and HTTP 206 with a valid FLAC header over IPv6.
A Rust request restricted to IPv6 independently confirmed the FLAC response.
This establishes a working address family for this check; it does not identify
Monochrome's internal reason for rejecting the IPv4 requests or promise that
every future IPv6 request will work.

Monochrome now attempts IPv6 separately before falling back to the normal
connection on transport failure. HTTP errors, including 429 cooldowns, retain
their existing handling. The decoder uses a temporary loopback CONNECT tunnel
to the verified IPv6 peer so FFmpeg retains the working route, original HTTPS
hostname and TLS connection. The tunnel accepts only that audio authority,
limits concurrent connections and closes with the decoder. A configured proxy
is not mistaken for an origin address. No browser cookies, tokens, Origin header
or browser user-agent impersonation are needed.

Input seeking produced extra failing byte-range requests during validation.
Monochrome instead decodes and discards audio up to the requested timestamp.
Large jumps can buffer longer because earlier frames must be fetched and decoded.

The Monochrome-only production-provider probe played Panorama muted for five
seconds, sought to 60 seconds and played five more seconds. It reported
**FLAC 24-bit / 48 kHz**, retained the original Deezer identity, and had no
fallback provider attached. It does not establish full-song continuity or
bit-perfect Windows output.

```powershell
cargo test --locked live_monochrome_audio_probe -- --ignored --nocapture
cargo test --locked monochrome_output_seek -- --ignored --nocapture
```

The first command defaults to Panorama; `TUITIFY_LIVE_TRACK` can select another
catalog identity. Neither probe writes saved queues or listening statistics.
Additional regressions cover IPv4-only service reachability, opaque tunnel
forwarding, cancellation, rejected tunnel destinations and the remaining
decoded duration after a seek.
