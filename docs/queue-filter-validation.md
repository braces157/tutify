# Queue filtering validation — 2026-10-01

This records the first build of the day. The subsequent bundle and installed
build are documented in [Listening tools validation](listening-tools-validation.md).

Queue now supports `/` or `f` to filter loaded track metadata by words across
title, artist, and album, ignoring letter case. Filtering is session-only and
does not replace the queue or change playback. Original queue positions are
retained, including duplicate occurrences and shuffled ordering. Missing
metadata is reported; matching rows refresh when metadata arrives. This is a
local filter over available metadata, not a scan of the entire Spotify catalog.

Enter finishes text entry; a subsequent Enter plays the selected match. Esc or
the clickable clear control restores the full queue. `.` clears the filter and
reveals the current track. Reorder moves one original queue position. Removing,
playing, album/artist navigation, Radio, context menus, and undo use the selected
queue occurrence. Empty results cannot trigger hidden-track actions, and clearing
the entire queue requires clearing the filter first.

## Checks

- `cargo fmt --check`: passed.
- `cargo test --locked`: 379 passed, 0 failed, 16 ignored. The ignored tests remain
  explicit opt-ins for live services, hardware, interactive checks, or benchmarks.
- `cargo clippy --all-targets --locked -- -D warnings`: passed without warnings.
- `cargo build --release --locked`: passed.
- Sixteen queue-filter tests cover Unicode text and paste limits, shortcut safety
  during editing, selection/navigation, duplicate occurrences, shuffled ordering,
  play/remove/reorder/undo, empty results, metadata refresh and F5 request offsets,
  menu invalidation, catalog back-navigation, and side-queue clicks.
- Render checks cover six themes at 32×10, 40×12, 60×18, 80×24, and 120×35. Matching
  rows, original-position mouse targets, clear controls, and long-query recovery
  remain usable. Compact queues reserve space for titles and visible rows.
- A native release executable ran in a real Windows PTY using isolated demo mode.
  Filtering `midnight` showed one of nine tracks at original row 8. The first Enter
  finished editing; the next played Midnight Tamarind. `K` moved it to row 7,
  `u` restored row 8 paused, Esc restored all nine rows, and `q` exited cleanly.

Full test output is in `work/queue-filter-tests.txt`. Live Spotify requests and
audible output from this new build were not exercised; demo verification used
fictional data and simulated playback.

## Installed build

The existing installer updated all five discovered launch copies: the project
root, `.cargo/bin`, `.local/bin`, the older `Programs/Tuitify/bin`, and the canonical
`%LOCALAPPDATA%/Programs/Tuitify` directory. Canonical installation ran last.
Each executable's SHA-256 matched `target/release/tuitify.exe`:

```text
5964978D0411263454A1064B1ED0C016DAEF6EEC638874934E3B65D24D21F4B1
```

`Get-Command tuitify -All`, `where.exe tuitify`, and the saved user PATH were checked.
Resolution using the saved machine/user PATH selected the canonical executable.
The canonical directory is first in saved user PATH, and unrelated entries were
preserved. Installation evidence is in `work/queue-filter-installation.txt`.

An existing canonical Tuitify process was left running throughout installation.
Quit and reopen it to load the new build; replacing the executable on disk does
not update an already running process.
