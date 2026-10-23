# Volta playback

Tuitify supports personal Volta applications through the
[official API](https://account.volta-music.com/apps/docs). It does not use browser
tokens, request signatures, or shared credentials: each user supplies their own
application.

## Create and connect an application

1. Create an application at <https://account.volta-music.com/apps> with the
   `catalog:read` and `stream:read` scopes.
2. To browse liked songs and saved playlists as well, also grant `library:read`
   and `playlists:read`.
3. Run `tuitify volta login`, then enter the client ID and the hidden secret.
4. Run `tuitify`.

Credentials are stored in **Windows Credential Manager**, outside the executable
and repository. For automation, `tuitify volta login --credentials-stdin` accepts
a bounded JSON object with `client_id` and `client_secret`; keep secrets out of
command arguments, logs, and files. Alternatively, set the
`TUITIFY_VOLTA_CLIENT_ID` and `TUITIFY_VOLTA_CLIENT_SECRET` environment variables.

`tuitify volta logout` removes the local credentials while preserving the queue.
Revoke the application on Volta's account page to invalidate its keys and tokens.

## What each mode does

| Mode | Catalog | Audio |
| --- | --- | --- |
| Plain `tuitify` (automatic) | Deezer public discovery, or your connected Spotify library | Monochrome → Volta → Spotify → YouTube, per song |
| Spotify + Volta | Spotify playlists, Liked Songs, albums, artists, radio, Smart Shuffle, Mix Builder | Volta, then a matched YouTube recording, then an existing Spotify Premium streaming login |
| `tuitify volta` (standalone) | Volta catalog with its own queue, cache, and settings | Volta, then a matched YouTube recording |
| `tuitify --source spotify` | Connected catalog | Spotify only |
| `tuitify --source youtube` | Connected catalog | YouTube only, even with Volta connected |

When Spotify is connected too, the **Spotify + Volta** catalog keeps your
playlists, Liked Songs, albums, artists, radio, Smart Shuffle and Mix Builder.
Connect Spotify through F6 Tools if needed. Your original Spotify queue, settings,
cache and statistics stay intact. Spotify Premium is required only for Spotify's
own audio backend.

## Matching a recording

Audio resolution uses the original recording's ISRC when available. Otherwise,
matching requires the same title including version, artist, and a compatible
duration; ambiguous results are rejected rather than guessed. Deezer and Spotify
track IDs, queue order, position and pause state stay intact, and the playback
badge identifies the audio provider.

In automatic mode each provider matches the original catalog recording
independently, so a Monochrome or Volta outage does not block the remaining
providers. Monochrome's public Tracks API supplies the first audio attempt without
an API key; rate limits and missing recordings advance to Volta, Spotify, then
YouTube. FFmpeg is required for Monochrome and Volta audio; missing tools are
prepared on automatic launch.

Confirmed Spotify-to-Volta recording matches are cached in memory for the session.

## Audio quality

Audio comes from the API's temporary playback link. Tuitify preserves the session
cookie, requests the highest lossless quality, and decodes with FFmpeg at the
reported sample rate (up to 192 kHz). Available source quality varies by track, and
Windows audio output can resample to its configured format. FFmpeg must be
installed; `tuitify youtube setup` installs the supported tools if necessary.

Now Playing identifies the audio provider; Now Playing and Discord presence show
the current stream's source quality, for example `FLAC 16-bit / 44.1 kHz`. The API
supplies bit depth and sample rate, and FFmpeg identifies the input codec. Only
reported values are shown; requesting lossless does not label an AAC fallback as
FLAC. Quality clears on track changes, failures and stop, and remains visible while
paused.

> The badge describes the **source stream**, not the Windows audio device's output
> format. Shared-mode output is not exclusive or bit-perfect.

For high-resolution playback, use a compatible DAC and select its desired
24-bit/high-rate default format in Windows Sound settings.

## Limits

Volta reads are throttled below 60/min and stream resolution below 10/min; access
tokens are reused until expiry. The standalone Volta catalogue uses artist search
for radio because the personal API does not expose the private radio routes.
Playback-device takeover and shared listening are not enabled. Saving likes uses
Spotify in combined mode and is unavailable in standalone Volta mode.

## Command reference

```powershell
tuitify volta                       # standalone Volta player
tuitify volta search "tuki"         # print Volta search results
tuitify volta probe volta:JPU902501324 --seconds 5   # muted decode/output check
tuitify volta probe <spotify-link>  # combined Spotify-to-Volta path
tuitify volta logout                # remove local credentials; keep the queue
```

`--volume 25` on a probe makes it audible. The probe does not change saved state.
For setup, `tuitify volta login` prompts for the client ID and the hidden secret;
`--credentials-stdin` reads both from JSON on standard input instead.

## Related reading

- [Monochrome playback validation](monochrome-playback-validation.md) — the first
  automatic audio attempt and FLAC header parsing
- [README](../README.md#install) — installation, automatic audio, and controls
- [Documentation index](README.md)
