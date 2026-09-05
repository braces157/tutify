# Tuitify

A personal Windows terminal player that streams Spotify audio directly through
librespot and Rodio/WASAPI. Spotify desktop can stay closed. Requires your Spotify
Premium account and a Spotify Developer app for catalog access.

## Run on Windows

Extract `Tuitify-0.1.0-windows-x86_64.zip` and open Windows Terminal in the extracted
folder. The executable needs no Rust installation. Use a standard font such as
Consolas or Cascadia Mono; icon fonts are not required.

```powershell
.\tuitify.exe auth --client-id YOUR_SPOTIFY_CLIENT_ID
.\tuitify.exe auth --streaming
.\tuitify.exe
```

For the first login:

1. Open the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Create an app for personal use, select Web API, and register this exact redirect:
   `http://127.0.0.1:8989/callback`.
3. Copy the **client ID**, not the client secret. Your account must be permitted to
   use the app. In development mode, the app owner needs an active Premium account;
   new apps ordinarily permit five users. See Spotify's current dashboard/rules.
4. Run the first command and finish browser authorization. No client secret or
   Spotify password is entered into Tuitify.
5. Run `auth --streaming` and use the **same Spotify account** in the browser.
   This second PKCE login uses librespot's streaming client identity and
   `http://127.0.0.1:8989/login`. Spotify labels this authorization **Spotify for
   Desktop**. It does not launch or require the desktop application. You do not
   register this second redirect in your Developer app.

**Why two logins?** Live validation found that the personal Developer app token
could authenticate a streaming session but Spotify rejected its audio-metadata
request. Librespot's streaming authorization resolved this. Your own client ID is
still used for every Web API catalog request. Both logins use PKCE, random state,
and Windows Credential Manager. This is an implementation change from the
original single-login assumption.

The callback listener binds only to `127.0.0.1:8989`, starts before the browser
opens, and times out after five minutes. If the browser shows a blank or blocked
callback page, check the terminal: a **Login saved** message confirms success.
Otherwise rerun the relevant auth command. Close any other process using port 8989.

To use `tuitify` without the `.exe` path, add its folder to your user PATH.

## Controls

| Key | Action |
| --- | --- |
| `1`–`5` | Search, Playlists, Liked Songs, Queue, Help |
| `Tab`, `Shift+Tab` | Switch focus between navigation and content |
| Up/Down or `k`/`j` | Move selection; scroll Help |
| `/` | Start a new search; names, Spotify track URLs, and track URIs work |
| Enter | Submit search, open a playlist, or play the selected track |
| Space | Pause/resume; retry failed playback from the saved position |
| `n` / `p` | Next / previous; previous restarts after three seconds |
| Left / Right | Seek backward / forward ten seconds |
| `+` / `-` | Adjust volume by five percent (`=` also increases) |
| `s` | Toggle shuffle, preserving the current occurrence |
| `r` | Cycle repeat off → queue → track |
| `a` | Append selected track without interrupting playback |
| Delete | Remove the selected Queue entry |
| Page Down | Fetch and append another catalog page |
| F5 | Refresh or retry catalog/queue metadata |
| Backspace | Return from playlist contents to playlists |
| `?` / F1 | Help |
| `q` / Ctrl+C | Save and quit |
| Esc | Cancel search editing, close Help, otherwise quit |

While entering a search, ordinary keys (including Space and `q`) enter text.
Submit with Enter before using playback shortcuts. Clipboard paste is supported.

Playing from a list replaces the local queue with its **loaded pages**, starting
at the selected track. Page Down loads more before playing. Explicitly unavailable
tracks are excluded. Appending does not move or restart the current track.
Shuffle randomizes the order with the current occurrence first; disabling it
restores list/insertion order. Appended songs go to the end of the current order.
Repeat track applies to completion; manually pressing next still advances.
Removing the playing entry stops playback instead of silently playing another.

At wider sizes the queue is also visible alongside the catalog. At narrow sizes
use `4` for the queue. Minimum useful size is 32 columns × 10 rows; 80 × 24 or
larger is recommended. Long names truncate to the available terminal width.

## Spotify API behavior

Search requests use `limit=10` and paginate with offsets. Playlists use
`GET /playlists/{id}/items`, including the renamed `item` response field. Liked
songs and playlists use pages of up to 50 items. Null, local-file, and podcast
entries are skipped. Missing optional metadata is tolerated.

Spotify development-mode apps can retrieve playlist contents only when the user
owns or collaborates on the playlist. A playlist can appear in your list while
its contents are restricted. Tuitify reports both HTTP 403 and metadata-only
responses with an explanation; it does not attempt to bypass restrictions.

See [Spotify's migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide)
for current rules, including changes made after February 2026.

## Persistence and privacy

`%LOCALAPPDATA%\Tuitify` contains:

- `config.json`: version, client ID, volume, shuffle, repeat.
- `queue.json`: version, track IDs, play order, current/selected queue indexes,
  and playback position in milliseconds.
- `instance.lock`: prevents two processes from racing on the same queue.

JSON writes use a flushed temporary file followed by same-volume atomic
replacement. Changes are saved asynchronously; playback position is checkpointed
every two seconds and flushed on normal exit. A hard kill can lose the last two
seconds. Settings and queue are separate atomic files, not a two-file transaction.
Startup always restores **paused**, without opening an audio device or starting
a stream. Queue names load into memory on demand.

OAuth access/refresh tokens are stored only in Windows Credential Manager under
Tuitify (`spotify-oauth` and `spotify-streaming-oauth`). The client ID is public
configuration; no client secret is required. Each explicit successful login
clears the old account queue; catalog login also removes the old streaming login.

There is no persistent metadata cache, listening history, analytics, or audio
download cache. Librespot uses temporary encrypted streaming buffers; normal
stream teardown removes them. No offline playback is provided. Operational
diagnostics retain only classified errors in RAM, not raw upstream URLs or tokens.

```powershell
.\tuitify.exe logout
```

Logout deletes both credentials and `queue.json`, retaining device settings and
the public client ID. Close the player before logging in or out.

If JSON is corrupted or from an unsupported version, Tuitify exits with its path
and preserves it. Move the affected file aside and start again to reset it. A
leftover uncommitted temporary write does not replace the previous valid snapshot.

## Troubleshooting

- **Login expired/revoked:** exit, run `auth` and then `auth --streaming`; for a
  streaming-only failure, use `auth --streaming`.
- **No audio:** select a working default output in Windows Sound settings, check
  Premium and volume, then retry with Space. No device error terminates the app.
- **Network loss:** browsing errors leave the queue intact. Press F5 to retry.
  If streaming stalls or the session disconnects, playback stops with an error;
  Space reconnects from the checkpoint. Loading is bounded to 45 seconds and
  playback without progress to 30 seconds; connection attempts to 35 seconds.
- **Quota:** Web API and refresh requests honor `Retry-After` (seconds or HTTP
  date) and suppress requests during cooldown. Tuitify does not repeatedly skip
  unavailable tracks or automatically retry failed catalog requests.
- **Unavailable track:** choose another or retry manually; no skipping loop.
- **Another instance:** quit the running player. The OS releases its lock even
  after a crash; the presence of `instance.lock` alone does not mean it is running.
- **Terminal failure:** the alternate screen, raw input, bracketed paste, and
  cursor are restored on normal exit, returned errors, and unwinding panics.
  Forced process termination cannot run cleanup; reopen the terminal if needed.

## Build and verify

Install stable Rust for `x86_64-pc-windows-msvc`, Visual Studio 2022 Build Tools
with **Desktop development with C++**, and the Windows SDK. This release was
built with Rust 1.95.0. Direct dependencies are pinned and `Cargo.lock` is tracked.

```powershell
cargo build --release --locked
cargo test --locked
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```

The executable is `target\release\tuitify.exe`. `scripts\release.ps1` runs checks,
builds, copies documentation, and creates a ZIP and SHA-256 manifest under `dist`.

The first-stage audio probe uses the same worker as the TUI:

```powershell
.\target\release\tuitify.exe probe spotify:track:4uLU6hMCjMI75M1A2tKUQC
```

Its controls are Space, Left/Right, +/-, and q. Close Spotify desktop first.
An additional **opt-in** live integration test plays audible audio and exercises
pause, seek, volume acknowledgement, injected session loss/reconnection, real
completion near the end of a track, and credential reuse:

```powershell
cargo test --locked live_streaming_acceptance -- --ignored --nocapture --test-threads=1
```

To verify terminal restoration on exit and a caught panic, run in a real terminal:

```powershell
cargo test --locked terminal_cleanup_acceptance -- --ignored --nocapture --test-threads=1
```

The intentional panic message is expected; the test must finish with `ok`.

Normal tests use mocked HTTP servers and do not require Spotify or audio. They
cover queue order and duplicate IDs, shuffle/repeat, completion handling, stale
completion rejection, persistence/corruption, OAuth state and PKCE, refresh
serialization/revocation, retry timing, pagination, restricted responses, and
rendering all views at normal, narrow, and tiny sizes.

## Source map and scope

`auth` handles PKCE and credentials; `catalog` handles the Web API; `playback`
owns streaming/audio with typed commands and events; `queue` owns order semantics;
`storage` owns disk state; `app` orchestrates asynchronous tasks; `ui` renders and
guards terminal state. Catalog requests, streaming, and file writes do not block
the UI loop.

This is personal-use software using unofficial librespot integration. Spotify
service changes can break playback. It is not affiliated with or approved by
Spotify. The [Spotify Developer Policy](https://developer.spotify.com/policy)
remains relevant.

V1 excludes playlist editing, podcasts, offline downloads, background services,
listening logs, AI, and song tier lists. The preference for a future cloud AI API
using your own key is recorded in `ROADMAP.md`; no AI key is requested or stored.

See `VALIDATION.md` for checks actually performed and remaining acceptance limits.
