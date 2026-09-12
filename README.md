<div align="center">
  <img src="docs/assets/brand/tuitify-logo-minimal-v2.webp" width="112" alt="Tuitify logo">

  # Tuitify

  **A fast, keyboard-first Spotify player for the Windows terminal.**

  Stream music without keeping the Spotify desktop app open. Browse your library,
  manage the queue, view synchronized lyrics, and control playback from one native TUI.

  [![Validate](https://github.com/braces157/tutify/actions/workflows/ci.yml/badge.svg)](https://github.com/braces157/tutify/actions/workflows/ci.yml)
  [![Latest release](https://img.shields.io/github/v/release/braces157/tutify?display_name=tag)](https://github.com/braces157/tutify/releases/latest)
  [![Platform](https://img.shields.io/badge/platform-Windows-0078D4)](#requirements)
  [![License](https://img.shields.io/github/license/braces157/tutify)](LICENSE)
</div>

---

Tuitify is a standalone Spotify client built with Rust, Ratatui, and librespot. It
plays audio directly through the Windows audio stack and keeps the whole listening
workflow inside Windows Terminal—no Electron shell and no background service.

> [!IMPORTANT]
> Spotify Premium is required for audio playback. Tuitify is an independent,
> personal-use project and is not affiliated with or endorsed by Spotify.

## Highlights

- **Direct playback** through librespot and WASAPI; Spotify Desktop can stay closed.
- **Library and catalog browsing** for playlists and Liked Songs, plus track
  search by title or artist and support for Spotify track links.
- **Powerful queue tools** including play next, reorder, remove, undo, shuffle,
  repeat, and Track Radio.
- **A responsive terminal UI** with keyboard and mouse support, five color themes,
  a semantic color system, restrained selection/focus states, a real-time FFT
  audio visualizer, and layouts that adapt to narrow terminals.
- **Synchronized lyrics** from LRCLIB with automatic scrolling.
- **Windows integration** for media keys, system media controls, metadata, and the
  playback timeline.
- **Discord Rich Presence** with track artwork and playback state; easy to disable.
- **Local listening statistics** with aggregate play counts and listening time—no
  analytics service or timestamped listening history.
- **Resilient sessions** with a persisted queue, atomic state writes, metadata
  caching, credential refresh, and paused-on-start restoration.

### Current release

The latest published download is **v0.2.7**. It adds a cohesive premium TUI,
safer queue/radio and Smart Shuffle updates, account-consistent authentication,
Unicode-safe search limits, Mix Builder pin recovery, and a reusable PATH
installer. It also includes Smart Shuffle, Mix Builder, the credential-free
native `demo` command, lyrics, the FFT visualizer, queue undo, Windows/Discord
integration, and local statistics.

## Requirements

- Windows 10 or 11 on x86-64
- [Windows Terminal](https://github.com/microsoft/terminal) or another terminal with
  modern color and mouse support
- For Spotify playback: a Premium account, network connection, and working
  Windows audio output device

No Rust installation is needed when using a release build. Consolas and Cascadia
Mono work out of the box; an icon font is not required.
Use 80 columns × 24 rows or larger; the minimum supported layout is 32 × 10.
At 32 × 10, Mix Builder shows the selected preview track, pin state, compact
apply/cancel controls, and a scrollable details shortcut; use a larger window to
inspect multiple preview rows comfortably.
The native demo needs no Spotify account or audio device.

## Install

1. Download `Tuitify-0.2.7-windows-x86_64.zip` from
   [release v0.2.7](https://github.com/braces157/tutify/releases/tag/v0.2.7).
2. Extract the archive.
3. Open Windows Terminal in the extracted folder and run:

```powershell
.\tuitify.exe
```

Tuitify opens the browser for guided sign-in on the first launch. Later launches
reuse the credentials stored in Windows Credential Manager.
Restored queues always start paused; press Space to resume.

To install the executable for your Windows user and add it to `PATH`, run:

```powershell
.\scripts\install.ps1
```

The installer copies `tuitify.exe` to `%LOCALAPPDATA%\Programs\Tuitify`, updates
the user `PATH` idempotently, and verifies the installed executable. Open a new
terminal afterward and run `tuitify` from any directory.

The release archive includes the same installer under `scripts`. You can also
keep the archive in a permanent folder and add that folder to your user `PATH`
manually.

### Try the native demo without Spotify

Run the actual terminal application in its isolated demo mode:

```powershell
.\tuitify.exe demo
```

For an executable on `PATH`, use `tuitify demo`. Demo data is session-only and
does not read or write the production queue, settings, statistics, recipes, or
credentials.

## Sign in

Tuitify uses two PKCE authorizations: one for Spotify catalog access and one for
librespot streaming. Both are browser-based, require the same Spotify account, and
never expose your password to Tuitify.

The default setup uses Spotatui's shared catalog client, so you do not need a client
secret or your own Spotify Developer application:

```powershell
.\tuitify.exe auth
```

The second authorization may be labelled **Spotify for Desktop** by Spotify. This
is the librespot streaming identity; it does not launch or require Spotify Desktop.

### Use your own Spotify application

If you prefer a personal Web API application:

1. Create an app in the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Enable Web API access and register `http://127.0.0.1:8989/callback` as a redirect URI.
3. Run the setup with the app's public client ID:

```powershell
.\tuitify.exe auth --client-id YOUR_SPOTIFY_CLIENT_ID
```

Do not supply a client secret. Spotify development-mode restrictions still apply;
the account may need to be explicitly allow-listed in your Developer Dashboard.

To replace a revoked login, use `auth --force`. To replace only the streaming
authorization, use `auth --streaming --force` and select the same account.
Close the player before running authentication or maintenance commands. Signing
back into the same verified catalog account preserves queue, cache, and statistics;
a different account or unknown prior identity clears those files. Both callbacks
listen only on `127.0.0.1:8989`; the shared client uses `/login`, while a personal
catalog app uses `/callback`.

## Controls

Press `?` or `F1` outside text entry for help. Mix Builder uses these keys for its
own details. While typing a search or recipe name, ordinary shortcut letters enter
text; finish text entry before using playback commands.

| Key | Action |
| --- | --- |
| `1`–`5` | Open Search, Playlists, Liked Songs, Queue, or Help |
| `Tab` / `Shift+Tab` | Move focus between navigation and content |
| `↑` / `↓` or `k` / `j` | Move the selection |
| `Enter` | Search, open, or play the selected item |
| `Space` | Pause, resume, or retry playback |
| `n` / `p` | Next / previous track |
| `←` / `→` | Seek backward / forward 10 seconds |
| `+` / `-` | Change volume by 5% |
| `m` | Mute or restore volume |
| `s` | Cycle shuffle off → shuffle → Smart Shuffle |
| `r` | Cycle repeat off → queue → track |
| `a` / `A` | Add selected item to queue / play next |
| `K` / `J` | Move the selected queue item up / down |
| `u` or `Ctrl+Z` | Undo the last queue edit |
| `R` | Start Track Radio from the selected track |
| `M` (`Shift+M`) | Open Mix Builder from Queue or an active/selected playlist |
| `l` / `v` / `S` | Toggle lyrics / visualizer / statistics |
| `t` | Cycle color themes |
| `/` or `f` | Filter loaded Liked Songs/playlist rows; `/` opens search from other views |
| `F2` / `F3` | Search Spotify / search your saved library |
| `F5` | Refresh catalog data or retry metadata |
| `Home` / `End` | Seek to the start / end of the track |
| `[` / `]` | Adjust volume by 1% outside Mix Builder |
| `C` / `Delete` | Clear the queue / remove its selected entry |
| `q` or `Ctrl+C` | Save and quit |

Windows media keys can control Play/Pause, Next, and Previous while Tuitify is
unfocused. Terminals that support mouse reporting can also select, scroll, seek,
toggle playback, and open context menus.

## Using Mix Builder

Open Queue with `4`, or open an accessible playlist with `2`, then press `M`
(`Shift+M`). The preview is separate from the live queue and playback until you
explicitly apply it.

- Press `3`, `4`, or `6` for a 30-, 45-, or 60-minute target.
- Press `[` or `]` to lower or raise the desired suggestion percentage, and `a`
  to cycle artist spacing.
- Move with `↑`/`↓`, press `?` for the full selected-track provenance and any
  limitations, and press `p` to pin a preview position.
- Press `g` to regenerate unpinned positions. In demo mode, the first suggestion
  request intentionally fails; `g` retries and demonstrates source-only recovery.
- Press `Enter` to replace the queue or `A` to append. Both are explicit and
  undoable with `u` or `Ctrl+Z`; `Esc` cancels without changing playback or queue.
- Press `w` to name and save a local recipe. Press `o` to cycle through and reopen
  saved recipes.

Duration, suggestion ratio, and artist spacing are preferences. Suggestion
percentages are measured by **track duration**, not song count. The builder shows
the achieved duration and ratio; `?` opens full details, and Up/Down scroll them.
Esc first closes details or text entry, then cancels the builder.

Source collection retains at most 25,000 track occurrences, with up to 500 pages
for playlists; incomplete or capped sources are labeled partial. Previews contain
at most 500 tracks. Recipes save a source and settings, not a fixed queue or pins;
reopening can fetch new data. Up to 100 recipes are stored, with an existing name
updated when you save it again. Replacement pauses playback; appending preserves it.

## Search scopes and queue behavior

- **Filter loaded rows:** `/` or `f` in Liked Songs or playlists searches only
  data already fetched into that view.
- **Spotify search:** F2 searches tracks across Spotify's catalog. Enter plays
  the selected result and starts Track Radio in the background.
- **Saved-library search:** F3 scans saved Liked Songs and accessible saved
  playlist tracks across pages. It needs network access and can take time.
  Esc cancels while retaining partial matches; F5 starts a fresh scan.

Playing from Liked Songs or a playlist replaces the queue with the loaded rows
(or filtered loaded rows). Load more pages before playing to include them, or
select a playlist and press `a` to append its pages asynchronously. Playing an
entry already in Queue keeps that queue. Playlist enqueues and saved-library scans
retain partial results after a later-page failure and offer explicit retries.

## How playback and recommendations work

Track Radio first requests Spotify recommendations and falls back to artist-based
search when that endpoint is unavailable. The UI identifies which source was
used; the fallback is not Spotify's personalized ranking algorithm. Smart
Shuffle mixes marked suggestions after every three original tracks while
preserving the playing and immediately upcoming entries.

Spotify limits some Web API endpoints for newer or development-mode applications.
Tuitify reports restricted playlist and recommendation responses instead of trying
to bypass them. See Spotify's
[Web API changes](https://developer.spotify.com/blog/2024-11-27-changes-to-the-web-api)
and [February 2026 migration guide](https://developer.spotify.com/documentation/web-api/tutorials/february-2026-migration-guide).

## Data and privacy

OAuth tokens are stored in **Windows Credential Manager**, not in project files.
Application state lives under `%LOCALAPPDATA%\Tuitify`:

| File | Contents |
| --- | --- |
| `config.json` | Public client ID and player preferences |
| `queue.json` | Queue order, selection, and saved position |
| `cache.json` | Bounded, expiring Spotify track metadata |
| `stats.json` | Aggregate play counts and listening time |
| `mix-recipes.json` | Named Mix Builder source and preference recipes |

Tuitify does not collect analytics, store your Spotify password, keep a timestamped
listening history, or provide an offline audio cache. Discord presence is enabled
by default: song details, artwork, and timing go to the local Discord client, which
may show them to other people according to your Discord privacy settings. To
disable it, close Tuitify, set `"discord_rpc": false` in `config.json`, and restart.

Opening lyrics sends the track title, artists, and duration to LRCLIB; Spotify
tokens are not sent there. Artwork may use Spotify's public oEmbed service when
catalog artwork is missing. These requests are separate from analytics.
Settings and queue changes are checkpointed asynchronously and restored paused.

Useful maintenance commands:

```powershell
# Remove cached metadata while keeping login and queue data
.\tuitify.exe clear-cache

# Remove credentials, queue, metadata cache, and listening statistics
.\tuitify.exe logout
```

Logout retains device settings and the public client ID. In the development
build, it also retains `mix-recipes.json`, including saved playlist names and IDs.
Remove that file manually with the player closed if you want to clear recipes.

## Troubleshooting

- **Expired/revoked login:** exit and run `tuitify auth --force`, or
  `tuitify auth --streaming --force` for a streaming-only problem.
- **No audio:** check Premium, volume, and the default Windows output; Space
  retries failed playback. A restored queue is paused until you resume it.
- **Network or catalog error:** F5 retries catalog/metadata requests. For HTTP
  429, wait for the reported cooldown instead of repeating authentication.
- **Restricted playlist:** Spotify may allow listing a playlist but restrict its
  contents to owners or collaborators in development mode.
- **Invalid config or queue:** Tuitify reports the path and preserves the file.
  Close the player and move it aside before restarting to reset that state.

## Build from source

Install stable Rust for `x86_64-pc-windows-msvc`, Visual Studio 2022 Build Tools
with **Desktop development with C++**, and the Windows SDK. Use current stable
Rust; this checkout was verified with Rust 1.95.0. The manifest declares 1.85,
but that minimum toolchain has not been verified in this review.

```powershell
git clone https://github.com/braces157/tutify.git
cd tutify
cargo build --release --locked
```

The executable is written to `target\release\tuitify.exe`.

Before contributing, run the same checks used by CI:

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

The static project website uses Node.js 22 or later:

```powershell
npm ci
npm run build
npm test
```

## Project docs

- [Architecture](ARCHITECTURE.md) — module boundaries, state ownership, and runtime design
- [Benchmarks](BENCHMARKS.md) — reproducible terminal rendering measurements
- [Validation](VALIDATION.md) — completed checks and remaining acceptance limits
- [Roadmap](ROADMAP.md) — planned work and explicit non-goals
- [Performance review](PERFORMANCE_REVIEW.md) — profiling findings and optimization notes
- [Refactor review](REFACTOR_REVIEW.md) — structural review and safeguards

## Scope

Tuitify is personal-use software built on unofficial librespot integration, so
Spotify service changes can break playback. It intentionally does not support
podcasts, offline downloads, playlist editing, background playback services, or
cloud listening analytics.

Released under the [MIT License](LICENSE).
