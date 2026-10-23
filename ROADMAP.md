# Deferred work and release history

Start with [docs/README.md](docs/README.md) for the documentation map. The current
code-backed implementation checklist is maintained locally in `FUTURE_PLAN.md` in
the development workspace, reviewed 2026-10-01. It includes priorities,
dependencies, implementation areas, completion criteria, and optional scope
expansions. The release history and explicit deferrals below remain useful
context; the checklist does not reverse those earlier choices.

This page is a history of shipped releases and deliberate deferrals. For what is
currently true, read [README.md](README.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

## Release history

| Version | Theme |
| --- | --- |
| 0.7.0 | Deezer discovery, continuous queue refill, prepared next-track audio, responsive layouts, audio-provider badges |
| 0.6.0 | Personal Volta playback, Spotify + Volta combined mode, source-quality labels |
| 0.5.0 | Automatic provider launch, in-app account connections, separate provider catalogs and queues |
| 0.4.0 | Typed service failures, backups and targeted recovery, doctor and redacted support reports, queue filtering, listening tools, build identity |
| 0.3.1 | Latin accent matching, duet lyrics, buffered persistence, statistics refresh, mouse controls |
| 0.3.0 | Glass theme, wallpaper backgrounds, native Windows Terminal profile |
| 0.2.9 | Related-artist discovery with verification, artist caps, seed retention |
| 0.2.8 | Album and artist browsing, breadcrumbs, search link resolution, demo catalog |
| 0.2.7 | Semantic TUI design system, queue invalidation, account-consistent reauthorization, PATH installer |
| 0.2.6 | Smart Shuffle, Mix Builder, queue undo, native offline demo |
| 0.2.5 | Local aggregate song statistics and the statistics overlay |
| 0.2.0 | Retro CRT themes, real-time FFT visualizer, LRCLIB lyrics, Track Radio, queue editing |
| 0.1.x | Search, playlists, Liked Songs, standalone playback, queue controls, persistence |

Per-release notes live in `docs/releases/` (for example
[the v0.7.0 notes](docs/releases/v0.7.0.md)) and in [CHANGELOG.md](CHANGELOG.md).
Verification evidence for each release is in [VALIDATION.md](VALIDATION.md).

## Delivered milestones

V0.2.0 delivered five retro CRT themes, a real-time FFT audio visualizer (up to
30 FPS while playing), synchronized lyrics from LRCLIB, explicit seed-based Track
Radio recommendations, and expanded queue manipulation (Play Next, reorder
up/down, clear queue).

V0.2.5 implemented local aggregate song statistics (play counts and cumulative
listened time per track) and a `Shift+S` overlay, fully offline and
privacy-preserving.

V0.2.6 delivered Smart Shuffle, the duration-targeted Mix Builder with inspectable
provenance, pinned deterministic regeneration and queue undo, plus an isolated
native demo that exercises the real terminal UI without Spotify credentials or
audio hardware.

V0.2.7 delivered a semantic, restrained TUI design system across all themes;
clear focus, selection, current-track, and playback-state hierarchy; safer queue
recommendation invalidation; account-consistent reauthorization; Unicode-safe text
limits; Mix Builder pin recovery; and a per-user PATH installer.

V0.2.8 delivered native album tracklist browsing with ordered track numbers,
durations, and playability status; artist top tracks discovery; dynamic breadcrumb
navigation with a history stack and `Esc` back-navigation (restoring prior view,
scroll offset, and selection); track context menu actions (**View Album** and
**View Artist**); instant Spotify search resolution for album and artist URLs/URIs
(including internationalized URLs and query parameters); and offline native demo
catalog datasets.

V0.2.9 upgraded Track Radio and Smart Shuffle to related-artist discovery with
verified tracks, stable seed retention across refills, recording-level
deduplication, per-artist caps, and artist-spacing heuristics. Recommendation
failures preserve the existing queue and remain visible for retry. Playback also
refreshes the Windows default output device while running, and the installer keeps
the canonical Tuitify directory first on the user PATH.

V0.3.0 added the Glass theme with custom or desktop wallpaper backgrounds, a
portable Unicode quadrant renderer, and a dedicated Windows Terminal profile for
native-resolution GPU rendering. Opaque selection and shortcut surfaces preserve
control contrast over artwork, while queue, lyrics, Unicode metadata, and real-time
FFT views share the same continuous background treatment.

V0.3.1 improved verified Latin accent matching in related-artist discovery and
primary-artist lyrics lookup for duets. It completed Statistics/Mix Builder mouse
controls, protected invalid saved history, buffered atomic JSON writes, avoided
duplicate snapshot clones, sped up statistics refresh, and corrected refresh
deadlines and spectrum sample-rate mapping.

V0.4.0 added typed service failures, session-scoped catalog observations, partial
library coverage, artist-source labels, account identity safeguards, versioned
backups and targeted recovery, offline diagnostics, redacted support reports,
queue filtering, listening tools, and compiled build identity.

V0.5.0 added automatic Spotify Premium/free YouTube Music launch, in-app account
connections, separate provider catalogs and queues, bounded parsed music caches,
and idle metadata-helper release.

V0.6.0 added personal Volta applications, standalone Volta saved state, Spotify
catalog/discovery with matched Volta audio, and source-quality labels in the TUI
and Discord presence.

V0.7.0 added public Deezer discovery, preserved catalog identities during audio
resolution, bounded automatic queue continuation, prepared Volta audio, matched
fallback, native output negotiation, and responsive player layouts.

## Deliberately deferred

Deferred by user choice: persistent timestamped listening history, AI suggestions,
and song tier lists. If an AI phase is pursued, the preferred integration is a
cloud API using the user's own key. Do not add listening collection or send
Spotify data to an AI provider without a separately agreed design and review of
Spotify policy.

Playlist editing, podcasts, offline downloads, background services, and cloud
listening analytics are outside the current scope. Deferral does not establish
that unofficial streaming or a future analysis feature is approved by Spotify.

ARM64 support remains subject to a dedicated build and hardware validation.
Installer rollback hardening remains deferred; the current installer verifies the
installed executable but does not implement transactional rollback.
