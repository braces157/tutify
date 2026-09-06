# Tuitify

A personal Windows terminal player that streams Spotify audio directly through
librespot and Rodio/WASAPI. Spotify desktop can stay closed. Requires a Spotify
account; Premium is required for playback. Catalog access uses Spotatui's shared PKCE
client by default and can be switched to a personal Spotify Developer app.

## Run on Windows

Extract `Tuitify-0.2.3-windows-x86_64.zip` and open Windows Terminal in the extracted
folder. The executable needs no Rust installation. Use a standard font such as
Consolas or Cascadia Mono; icon fonts are not required.

```powershell
.\tuitify.exe
```

For the first login, run `tuitify`. Tuitify follows Spotatui's PKCE flow and uses its
shared catalog client by default, so no client secret or Developer app is required.
The browser opens automatically and the callback listener is ready before it does.
After the catalog authorization, Tuitify opens the second authorization. Use the
**same Spotify account**.

If you prefer your own Web API app (for a private app allow-list or separate quota):

1. Open the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Create an app for personal use, select Web API, and register this exact redirect:
   `http://127.0.0.1:8989/callback`.
3. Run `tuitify auth --client-id YOUR_SPOTIFY_CLIENT_ID`. Paste the **client ID**, not
   the client secret. In development mode, the app owner needs an active Premium
   account and the Spotify account must be allowed to use the app.
4. Finish the browser authorization. Tuitify automatically opens the second
   authorization. Use the same Spotify account there as well.

The shared catalog client uses `http://127.0.0.1:8989/login`; you do not register
that redirect in your own app. No Spotify password is entered into Tuitify.

The second PKCE login uses Spotatui/librespot's streaming client identity and
`http://127.0.0.1:8989/login`. Spotify labels this authorization **Spotify for
Desktop**. It does not launch or require the desktop application. You do not
register this second redirect in your Developer app.

The player opens automatically when setup finishes. Later launches reuse saved
credentials without browser prompts or extra profile checks. If you complete only
the first login, the next launch resumes at the missing second step.

`tuitify auth` runs the same guided setup without opening the player. Add
`--client-id YOUR_SPOTIFY_CLIENT_ID` to select a personal catalog app. Repeating
`auth` reuses saved logins. Use `tuitify auth --force` to replace them, or
`tuitify auth --streaming --force` to replace only the streaming login. These explicit
replacements clear the queue.

**Why two logins?** Live validation found that the personal Developer app token
could authenticate a streaming session but Spotify rejected its audio-metadata
request. Librespot's streaming authorization resolved this. The selected catalog
client ID is used for every Web API catalog request. Both logins use PKCE, random state,
and Windows Credential Manager. This is an implementation change from the
original single-login assumption.

The callback listener binds only to `127.0.0.1:8989`, starts before the browser
opens, and times out after five minutes. After authorization, the terminal reports
token exchange and account verification progress. The callback page waits for the
actual result: it confirms saved credentials or displays the setup error. Return
to the original terminal to continue setup. If the browser
shows a blank or blocked callback page, the terminal's **Login saved** message
confirms success. Close any other process using port 8989.

HTTP 429 is a Spotify rate limit, not evidence of a Premium problem. Verification
waits for `Retry-After` and retries once when the delay is at most 30 seconds.
For longer delays, or a second rate limit, it displays the required wait and exits.
Avoid repeating login attempts during that wait. A saved catalog account ID is
reused when checking the streaming login to avoid an unnecessary profile request.

To use `tuitify` without the `.exe` path, add its folder to your user PATH.

## Controls

Mouse controls (v0.2.2, in terminals with mouse reporting such as Windows Terminal):

- Left-click a song/playlist to select it, or a navigation label to switch views.
- Right-click a song for **Play**, **Add to queue**, or **Play next**. Queue rows
  also offer **Remove from queue**; playlists offer **Open** and **Add playlist**.
- Scroll the wheel over a list to move three rows; Help and plain lyrics scroll too.
- Click the playing/paused badge to toggle playback, or the progress bar to seek.
- Click outside the menu or press Esc to dismiss it; Up/Down and Enter also work.
- Mouse capture prevents right-click from pasting clipboard text into the filter.
  Use Ctrl+Shift+V for an intentional terminal paste into the search/filter field.
  Mouse capture is released on normal exit and unwinding panic. Keyboard controls
  remain available in terminals that do not send mouse events.


| Key | Action |
| --- | --- |
| `1`–`5` | Search, Playlists, Liked Songs, Queue, Help |
| `Tab`, `Shift+Tab` | Switch focus between navigation and content |
| Up/Down or `k`/`j` | Move selection; scroll Help |
| `/` or `f` | Filter loaded Liked Songs/playlist rows; `/` edits the query in Search |
| F2 | Search Spotify's catalog (songs, artists, or a Spotify track link) |
| F3 | Search all saved Liked Songs and saved playlist tracks |
| Enter | Submit search, open a playlist, or play the selected track |
| Space | Pause/resume; retry failed playback from the saved position |
| `n` / `p` | Next / previous; previous restarts after three seconds |
| Left / Right | Seek backward / forward ten seconds |
| Home / End | Jump to the start / end of the track |
| `+` / `-` | Adjust volume by five percent (`=` also increases) |
| `[` / `]` | Adjust volume by one percent for fine control |
| `m` | Mute or restore the previous volume |
| `s` | Toggle shuffle, preserving the current occurrence |
| `r` | Cycle repeat off → queue → track |
| `t` | Cycle retro color themes (Classic, Phosphor Green, Amber, Mono, Cyberpunk) |
| `v` | Toggle decorative retro visualizer (up to 30 FPS while playing) |
| `l` | Toggle live synchronized lyrics (Lrclib auto-scroll) |
| `R` | Track Radio: queue related recommendations for selected song |
| `a` | Append selected track, or fetch and append an entire playlist |
| `A` | Play Next: insert selected track directly after current track |
| `K` / `J` | Move selected track up / down in Queue |
| `C` | Clear the entire queue |
| `u` / Ctrl+Z | Undo a queue edit; restore the previous queue paused |
| `.` / `c` | Jump cursor to currently playing track in Queue |
| Delete / `d` / `x` | Remove the selected Queue entry |
| Page Down | Fetch and append another catalog page (infinite pagination) |
| F5 | Refresh/retry catalog or queue metadata; restart saved-library search |
| Backspace | Return from playlist contents to playlists |
| `?` / F1 | Help |
| `q` / Ctrl+C | Save and quit |
| Esc | Close overlay/menu, clear filter, cancel an active library scan, or quit |

The playback bar continuously shows elapsed time, total duration, percentage
complete, and remaining time. Left/Right seeks while the track is loaded; Home
and End jump to exact boundaries. Volume changes are reflected immediately in
the `VOL` indicator, and `m` temporarily mutes without losing the previous level.

While entering a search, ordinary keys (including Space and `q`) enter text.
Submit with Enter before using playback shortcuts. Clipboard paste is supported.

Playing from a list replaces the local queue with its **loaded pages**, starting
at the selected track. Page Down loads more before playing. Search follows the same list behavior;
press `R` explicitly to replace the queue with a track and related recommendations. Explicitly unavailable
tracks are excluded. Appending does not move or restart the current track.
Shuffle randomizes the order with the current occurrence first; disabling it
restores list/insertion order. Appended songs go to the end of the current order.
Repeat track applies to completion; manually pressing next still advances.
Removing the playing entry stops playback instead of silently playing another.

At wider sizes the queue is also visible alongside the catalog. At narrow sizes
use `4` for the queue. Minimum useful size is 32 columns × 10 rows; 80 × 24 or
larger is recommended. Long names truncate to the available terminal width.

The queue supports up to 100,000 entries. Playlist enqueue follows pages in the
background, shows the number actually added, and retains partial additions if a
later page fails. Clearing or replacing the queue cancels its pending playlist
and radio jobs; late responses cannot refill an unrelated queue.

### Queue recovery and search (v0.2.3)

**Preserve the queue after re-login.** After a successful catalog login, Tuitify
compares the verified Spotify account with the previously saved account. Signing
back into the **same account** preserves the queue and metadata cache, including
queue order and saved playback position. A different account, or an unknown prior
identity, clears the old account's queue/cache. Streaming reauthorization must
match the catalog account and does not clear these files. Explicit `logout` still
removes both credentials and the queue/cache.

**Undo queue actions.** Press `u` or Ctrl+Z to recover from accidental removal,
clearing, or queue replacement. Undo also covers adding tracks/playlists, reordering,
shuffle changes, and starting radio. The previous order, current track, position,
and shuffle setting are restored **paused**; press Space to resume. Pending queue
jobs are cancelled so late playlist/radio results cannot overwrite the restoration.
Right-click in the Queue view for **Undo queue change**, including when the queue
is empty. History lasts only for the current session and retains at most ten
snapshots with a combined limit of 100,000 track IDs; older snapshots are dropped.

**Choose the search scope explicitly:**

| Mode | What it searches | How to use it |
| --- | --- | --- |
| Filter loaded rows | Only Liked Songs or playlist rows already fetched into the current view | Press `/` or `f`; the field is labeled **FILTER LOADED**. |
| Spotify search | Spotify's catalog, including music outside your saved library | Press F2 or click **F2 Spotify**, enter a query, then Enter. |
| Saved-library search | All pages of saved Liked Songs and saved playlist tracks that Spotify allows the app to read | Press F3 or click **F3 Saved library**, enter a query, then Enter. |

For example, filtering 50 loaded songs cannot find a saved song on a later page.
F3 scans those later pages and the saved playlists as well. Choosing F2/F3 from an
active filter carries its text into the selected search mode. Saved-library search
matches title or artist text without case sensitivity and deduplicates track IDs.
It fetches pages sequentially, with 200 ms between requests, and displays matches
and a scanned-track count as results arrive. Scanned counts include duplicate
occurrences across playlists; matching songs appear only once.

Press Esc to cancel a scan while browsing its results; partial matches remain.
If editing the query, Esc first leaves the input. F5 starts a fresh scan. Network
errors, inaccessible playlists, rate limits, or traversal limits leave results
explicitly marked **partial**, rather than claiming the whole library was searched.
A scan is capped at 100,000 unique tracks, 100,000 playlists, and 100,000 page
requests. Large libraries can take time and require network access; this is not
an offline library index.

The visualizer is a decorative animation, not audio-frequency analysis. The
unchanged paused screen does not animate or continuously redraw. Long queue and
catalog views build only visible rows, and filters reuse their results until
rows or filter text change.

Lyrics are requested from **Lrclib only when you open the lyrics view** and real
track metadata is available. Requests send the track title, artist names, and
duration to `lrclib.net`; no Spotify tokens are sent there. Lyrics are kept in
memory. F5 retries failures. Plain lyrics scroll with Up/Down or Page Up/Down.

## Spotify API behavior

Search requests use `limit=10` and paginate with offsets. Restored queue metadata
loads current/visible entries first, using at most five individual track requests
at a time. Failures stop further hydration until F5; there is no automatic retry loop. Playlists use
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

- `config.json`: version, client ID, volume, shuffle, repeat, and theme.
- `queue.json`: version, track IDs, play order, current/selected queue indexes,
  and playback position in milliseconds.
- `cache.json`: versioned track names, artists, duration, availability, and metadata
  fetch timestamps; at most 3,000 entries, expiring after 24 hours.
- `instance.lock`: prevents two processes from racing on the same queue.

JSON writes use a flushed temporary file followed by same-volume atomic
replacement. Changed settings, queue, and metadata are checkpointed asynchronously every two
seconds and flushed on normal exit. Each file has its own coalescing background
writer; unchanged files are not rewritten. Failed writes are reported and retried
at the next checkpoint. A hard kill can lose changes since the last successful
checkpoint, normally about two seconds. These are separate atomic files, not a
multi-file transaction.
Startup always restores **paused**, without opening an audio device or starting
a stream. Queue names load into memory on demand.

OAuth access/refresh tokens are stored only in Windows Credential Manager under
Tuitify (`spotify-oauth` and `spotify-streaming-oauth`). The client ID is public
configuration; no client secret is required. A successful catalog re-login to the
same verified account preserves the queue and metadata cache. A changed or unknown
prior account clears those files. Catalog login replaces the streaming login;
streaming reauthorization must use the same account and preserves queue/cache.

The metadata cache retains tracks encountered during browsing and playback. It
is not a listening log: it stores no playback times or play counts. It contains no
credentials. There is no listening-history log, analytics, or audio download cache.
Librespot uses temporary encrypted streaming buffers; normal
stream teardown removes them. No offline playback is provided. Operational
diagnostics retain only classified errors in RAM, not raw upstream URLs or tokens.

```powershell
.\tuitify.exe logout
```

Logout deletes both credentials, `queue.json`, and `cache.json`, retaining device
settings and the public client ID. Explicit account replacement also clears the
cache. Run `tuitify clear-cache` to remove cached metadata without logging out.
Close the player before logging in, logging out, or clearing its cache.

If config or queue JSON is corrupted or from an unsupported version, Tuitify exits with its path
and preserves it. Move the affected file aside and start again to reset it. A
leftover uncommitted temporary write does not replace the previous valid snapshot.
An old or invalid metadata cache is ignored with a status message and rebuilt as
names load. F5 invalidates current/visible queue metadata and retries failed
metadata and lyrics requests.

## Troubleshooting

- **Login revoked:** exit and run `tuitify auth --force`; for a streaming-only
  failure, use `tuitify auth --streaming --force`. Expired access tokens refresh automatically.
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

Website assets and browser tests use Node.js 22 or later:

```powershell
npm ci
npm run build:css
npm test
```

The generated stylesheet is committed for static hosting; the website needs no
runtime Tailwind CDN or font service. See [BENCHMARKS.md](BENCHMARKS.md) for the
reproducible offline rendering benchmark and limits of those measurements.

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
