# Audio quality review — 2026-10-23

Tuitify already requests Monochrome FLAC, Volta `lossless_hq`, Spotify 320 kbps,
and YouTube's best available HTTPS audio stream. These are requests, not promises
about every recording: provider fallbacks can be lossy. The displayed quality
describes reported or decoded source audio, separately from Windows output.

The HTTP pipeline decodes into stereo 32-bit floating point PCM. It preserves
24-bit FLAC samples exactly at matching rates and uses FFmpeg's bandlimited
resampler for rate changes. Spotify uses a separate streaming sinc resampler.
Spotify normalization is disabled in the pinned player's default configuration.

## Findings and fixes

1. Spotify's previous 16-tap resampler attenuated upper treble when converting
   44.1 kHz audio. It now uses 64 taps, 1024 fractional phases and double precision
   accumulation. Matching-rate audio still bypasses the resampler.
2. HTTP decoders previously clipped floating point samples to full scale before
   app volume was applied. Lossy decoding and resampling can produce legitimate
   peaks beyond that range. These peaks now survive volume control, with
   nonfinite values silenced. Floating point output retains this headroom;
   integer output clips any remaining overload at the final conversion. For
   example, a decoded peak of 1.25 now becomes 0.625 at 50%
   volume, rather than being distorted to 0.5.

Tests were first run against the previous behavior and failed, reproducing both
issues. The measurements below use generated tones, not an acoustic recording
or a listening comparison.

| Measurement | Previous | Updated |
| --- | ---: | ---: |
| 18 kHz response, 44.1 → 384 kHz | −0.47663 dB | +0.00055 dB |
| 20 kHz response, 44.1 → 384 kHz | −2.09805 dB | −0.00107 dB |
| 20 kHz tone alias at 12 kHz, 44.1 → 32 kHz | −25.08 dB | −84.93 dB |

Response tests also cover 48 and 96 kHz output at 1, 12, 18 and 20 kHz.
Existing tests cover equal-rate transparency, constant-signal gain, stereo
channels and irregular chunk boundaries.

An optimized harness using the actual resampler processed five seconds of audio
at 384 kHz output in approximately 116 ms on this computer (43 times real time).
This measures the resampler alone, excluding decoding and Windows playback.

## Windows output and remaining limits

The current default Windows mix format reports 384000 Hz, stereo, F32. Silent
output checks requested 44.1, 48, 96, 192 and 384 kHz; every request opened at
384 kHz and drained successfully. Tuitify uses the actual negotiated rate, so
the decoder and output agree. F32 describes the Windows mix format; it does not
establish the DAC's physical resolution, headphone performance, or audibility.

The pinned CPAL WASAPI backend opens shared-mode streams. Consequently Tuitify
does not promise bit-perfect delivery to the DAC: Windows volume, mixing and
device effects remain part of the path. An optional exclusive output backend
with source-rate negotiation would be a further feature, with separate driver,
device switching and recovery testing. The current changes retain shared mode.
Microsoft documents the distinction in
[Device Formats](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-formats)
and [GetMixFormat](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getmixformat).

Spotify playback in this build remains lossy, with a 320 kbps preference and
supported lower-quality fallbacks. YouTube playback remains limited by the
selected encoded stream. A high Windows output rate cannot recover information
missing from a source recording or its encoding.

## Reproduce

Run from the repository root in PowerShell:

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked playback::tests::resampler_ -- --nocapture
cargo test --locked audio_output::tests::live_output_opens_verified_rate_and_drains_silence -- --ignored --nocapture
cargo test --locked volta::playback::tests::real_flac_decode_preserves_native_rate_and_sends_cookie -- --ignored --nocapture
cargo test --locked volta::playback::tests::high_resolution_downsampling_rejects_aliasing_and_preserves_duration -- --ignored --nocapture
cargo test --locked volta::playback::tests::monochrome_output_seek_preserves_remaining_duration -- --ignored --nocapture
```

The last three tests require installed FFmpeg and use generated FLAC over loopback
HTTP. They do not access a music account or play audible audio. The output test
opens the default device with silence. These checks do not measure the analog
output or establish that every quality improvement is audible.
