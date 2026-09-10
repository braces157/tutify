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
- **Library and catalog browsing** for playlists, Liked Songs, tracks, artists, and
  Spotify track links.
- **Powerful queue tools** including play next, reorder, remove, undo, shuffle,
  repeat, Track Radio, and Smart Shuffle suggestions.
- **A responsive terminal UI** with keyboard and mouse support, five color themes,
  a playback visualizer, and layouts that adapt to narrow terminals.
- **Synchronized lyrics** from LRCLIB with automatic scrolling.
- **Windows integration** for media keys, system media controls, metadata, and the
  playback timeline.
- **Discord Rich Presence** with track artwork and playback state; easy to disable.
- **Local listening statistics** with aggregate play counts and listening time—no
  analytics service or timestamped listening history.
- **Resilient sessions** with a persisted queue, atomic state writes, metadata
  caching, credential refresh, and paused-on-start restoration.

## Requirements

- Windows 10 or 11 on x86-64
- [Windows Terminal](https://github.com/microsoft/terminal) or another terminal with
  modern color and mouse support
- A Spotify account; Premium is required for playback
- A working Windows audio output device

No Rust installation is needed when using a release build. Consolas and Cascadia
Mono work out of the box; an icon font is not required.

## Install

1. Download `Tuitify-0.2.5-windows-x86_64.zip` from the
   [latest release](https://github.com/braces157/tutify/releases/latest).
2. Extract the archive.
3. Open Windows Terminal in the extracted folder and run:

```powershell
.\tuitify.exe
```

Tuitify opens the browser for guided sign-in on the first launch. Later launches
reuse the credentials stored in Windows Credential Manager.

To call `tuitify` from any directory, add the extracted folder to your user `PATH`.

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

## Controls

Press `?` or `F1` at any time for the complete in-app reference.

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
| `l` / `v` / `S` | Toggle lyrics / visualizer / statistics |
| `t` | Cycle color themes |
| `/` or `f` | Filter the current collection |
| `F2` / `F3` | Search Spotify / search your saved library |
| `F5` | Refresh catalog data or retry metadata |
| `q` or `Ctrl+C` | Save and quit |

Windows media keys can control Play/Pause, Next, and Previous while Tuitify is
unfocused. Terminals that support mouse reporting can also select, scroll, seek,
toggle playback, and open context menus.

## How playback and recommendations work

Playing a result replaces the local queue and starts audio in Tuitify. Track Radio
first requests Spotify recommendations and falls back to artist-based search when
that endpoint is unavailable. Smart Shuffle mixes marked suggestions into the
existing queue while preserving original entries.

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

Tuitify does not collect analytics, store your Spotify password, keep a timestamped
listening history, or provide an offline audio cache. Discord presence is sent only
to the local Discord client and can be disabled by setting `"discord_rpc": false`
in `config.json`.

Useful maintenance commands:

```powershell
# Remove cached metadata while keeping login and queue data
.\tuitify.exe clear-cache

# Remove credentials, queue, metadata cache, and listening statistics
.\tuitify.exe logout
```

## Build from source

Install stable Rust for `x86_64-pc-windows-msvc`, Visual Studio 2022 Build Tools
with **Desktop development with C++**, and the Windows SDK. The crate requires
Rust 1.85 or newer.

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
