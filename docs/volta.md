# Volta playback

Tuitify supports personal Volta applications through the [official API](https://account.volta-music.com/apps/docs). It does not use browser tokens, request signatures, or shared credentials.

Create your own application at https://account.volta-music.com/apps with `catalog:read` and `stream:read`. To browse liked songs and saved playlists, also grant `library:read` and `playlists:read`.

Run `tuitify volta login`, enter its client ID and hidden secret, then run `tuitify`. Automatic audio tries **Monochrome → Volta → Spotify → YouTube**, skipping unavailable connections. When Spotify is connected too, the **Spotify + Volta** catalog retains your playlists, Liked Songs, albums, artists, radio, Smart Shuffle and Mix Builder. Connect Spotify through F6 Tools if needed. Your original Spotify queue, settings, cache and statistics stay intact. Spotify Premium is required only for Spotify's own audio backend.

The Volta credentials are stored locally in Windows Credential Manager, outside the executable and repository. Alternatively, use the process environment variables `TUITIFY_VOLTA_CLIENT_ID` and `TUITIFY_VOLTA_CLIENT_SECRET`. Each user supplies their own application.

Press `/` to search, `2` to browse Spotify playlists, or `3` for Liked Songs, then Enter to play. Spotify track IDs and the existing discovery algorithm remain intact. Audio resolution uses the original recording's ISRC when available. Otherwise, matching requires the same title including version, artist and compatible duration; ambiguous results are rejected. Each new song tries Monochrome first, then connected Volta, an existing Spotify Premium streaming login, and installed YouTube tools. Queue identities stay intact, and the playback badge identifies the audio provider. `tuitify --source spotify` opens Spotify audio explicitly.

Deezer supplies the default public discovery catalog in each mode. Monochrome's public Tracks API supplies the first audio attempt without an API key; rate limits and missing recordings advance to Volta, Spotify, then YouTube. Explicit `--source youtube` stays on YouTube even with Volta connected. `tuitify volta` explicitly opens standalone Volta mode with its separate queue, cache and settings. FFmpeg is required for Monochrome and Volta audio; missing tools are prepared on automatic launch.

In automatic mode, each provider matches the original catalog recording independently, so a Monochrome or Volta outage does not block the remaining providers. The queue identity, position and pause state stay intact across retries. Explicit Volta mode retains its existing YouTube and Spotify fallbacks.

Audio comes from the API's temporary playback link. Tuitify preserves the session cookie, requests the highest lossless quality, and decodes with FFmpeg at the reported sample rate (up to 192 kHz). Available source quality varies by track; Windows audio output can resample to its configured format. FFmpeg must be installed; the existing `tuitify youtube setup` command installs the supported tools if necessary.

Now Playing identifies the audio provider; Now Playing and Discord presence show the current stream's source quality, for example `FLAC 16-bit / 44.1 kHz`. The API supplies bit depth/sample rate and FFmpeg identifies the input codec. Only reported values are shown; requesting lossless does not label an AAC fallback as FLAC. Quality clears on track changes, failures and stop, and remains visible while paused. The badge describes the source stream, not the Windows audio device's output format.

Volta reads are throttled below 60/min, stream resolution below 10/min, and access tokens are reused until expiry. Confirmed Spotify-to-Volta recording matches are cached in memory for the session. The standalone Volta catalogue uses artist search for radio because the personal API does not expose the private radio routes. Playback-device takeover and shared listening are not enabled. Saving likes uses Spotify in combined mode; it is unavailable in standalone Volta mode.

`tuitify volta search "tuki"` prints Volta search results. `tuitify volta probe volta:JPU902501324 --seconds 5` tests decoding and output muted; a Spotify track link or 22-character track ID tests the combined Spotify-to-Volta path. Add `--volume 25` to hear it. `tuitify volta logout` removes the local credentials while preserving the queue. Revoke the application on Volta's account page to invalidate its keys and tokens.

For automation, `tuitify volta login --credentials-stdin` accepts a bounded JSON object with `client_id` and `client_secret`. Keep secrets out of command arguments, logs, and files.
