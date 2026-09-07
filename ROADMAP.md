# Deferred work

V1 ships search, playlists, liked songs, standalone playback, local queue controls,
and settings/queue persistence.

V0.2.0 delivers 5 retro CRT themes, decorative retro visualizer (up to 30 FPS while playing),
real-time synchronized lyrics (Lrclib), explicit seed-based Track Radio recommendations,
and expanded queue manipulation (Play Next, reorder up/down, clear queue).

V0.2.5 implements local aggregate song statistics (play counts and cumulative listened time per track)
and a Shift+S overlay, fully offline and privacy-preserving.

Deferred by user choice: persistent timestamped listening history, AI suggestions, and song
tier lists. If an AI phase is pursued, the preferred integration is a cloud API
using the user's own key. Do not add listening collection or send Spotify data to
an AI provider without a separately agreed design and review of Spotify policy.

Playlist editing, podcasts, offline downloads, background services, and cloud listening
analytics are outside v1. Deferral does not establish that unofficial streaming
or a future analysis feature is approved by Spotify.
