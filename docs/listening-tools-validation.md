# Listening tools and everyday controls validation — 2026-10-01

This update adds practical controls for a long listening session: an F6 tools
menu with timed and end-of-track sleep, undoable queue cleanup, Ctrl+Enter Play
Next, recent-search recall, and Ctrl+R queue-filter recovery. It includes the
queue filtering introduced earlier on the same day.

## Behavior

- Sleep presets are 15, 30, 45, and 60 minutes. A visible countdown uses wall
  time, including time spent paused. Expiry pauses playback while retaining the
  queue, volume, and position; Space resumes. Cancel leaves playback unchanged.
- Stop after current track requires a loaded track, takes precedence over Repeat,
  and leaves the same occurrence paused at its start. Changing or restarting a
  track cancels that mode. Timers are session-only.
- Cleanup removes played entries, upcoming duplicates by exact track ID, or
  entries with known unavailable metadata. It operates on the full queue,
  preserves the current occurrence, playback position, shuffle ordering and
  surviving suggestion identities, and retains unknown availability. Existing
  undo restores a queue snapshot in the paused state.
- Ctrl+Enter adds a catalog track next or promotes the selected existing queue
  occurrence without inserting a copy or interrupting playback.
- Up/Down in the search editor recalls the last 20 unique submitted queries and
  restores the unsent draft. Recall alone does not submit a network request.
- Ctrl+R outside text entry restores the last cleared Queue filter.
- The tools menu supports mouse selection and explicit Apply/Close controls,
  isolates underlying clicks, and keeps its selected option visible at 32×10.

## Checks

- `cargo fmt --check`: passed.
- `cargo test --locked`: 396 passed, 0 failed, 16 ignored. Ignored tests remain
  explicit opt-ins for live services, hardware, interactive checks, or benchmarks.
  Output is in `work/listening-tools-tests.txt`.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- `cargo build --release --locked`: passed for the installed executable.
- Seventeen added tests cover timer presets/cancellation, expiry during playing,
  paused and loading states, late playback events, generation changes, Repeat,
  shuffled queue cleanup, availability, undo, filtered Play Next, mouse controls,
  search recall/drafts, filter recovery, and compact rendering in all six themes.
- The native 80-column walkthrough found a clipped Quit hint. After fixing it,
  the two listening UI tests and Clippy were rerun successfully, the release was
  rebuilt, and a new native demo showed the complete F6/Help/Quit hints.
- `git diff --check`: passed.

## Native walkthrough

A real Windows PTY ran the release executable in isolated demo mode with
fictional data and simulated playback. The walkthrough verified:

1. F6 opens the nine-option menu; setting a 15-minute timer displays its countdown,
   and cancelling removes it without changing playback.
2. Stop after current track, followed by seeking to the end, stops on the same
   track at position zero and removes the timer instead of advancing.
3. Removing played queue entries reduces the queue from nine to six while the
   current track continues. Undo restores nine entries in the paused state.
4. Clearing a `paper` Queue filter and pressing Ctrl+R restores its two matches.
5. After submitting `neon` and `paper`, Up recalls each query and Down restores
   the unfinished `draft` text without submitting it.
6. Both demo processes exit cleanly with q.

Live Spotify requests and audible output from this new build were not exercised.

## Installed build

The existing `scripts/install.ps1` updated all five discovered launch copies:
the project root, `.cargo/bin`, `.local/bin`, the older `Programs/Tuitify/bin`,
and the canonical `%LOCALAPPDATA%/Programs/Tuitify` directory. Canonical
installation ran last. All five SHA-256 hashes match `target/release/tuitify.exe`:

```text
6E5265532D83A0685F2C28C922E79615B914425E8FFF3D2D2C6341AF0C112FAD
```

`Get-Command tuitify -All`, `where.exe tuitify`, saved user PATH, and running
process executable paths were inspected. Resolution using the saved machine/user
PATH selected the canonical executable. Its directory is first in saved user
PATH; unrelated PATH entries were preserved. Evidence is in
`work/listening-tools-installation.txt`.

No production Tuitify process was stopped. No Tuitify process remained at the
installation verification. If an older instance is still open in another
session, quit and reopen it to load this build.
