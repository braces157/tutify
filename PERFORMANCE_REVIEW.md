# Website and TUI review — 2026-09-06

The largest performance issue is repeated work in the TUI render loop. The largest reliability issues are silent metadata failures and background responses that outlive the queue they were requested for. The website needs compiled CSS, accessible demo controls, and evidence for its performance claims.

The findings below describe the original review snapshot; their line numbers and baseline results are historical. The subsequent local implementation addresses all 18 findings and the additional improvements. P1 means fix first because the effect is substantial; P2 means a concrete defect or meaningful improvement; P3 means polish.

## Implementation follow-up

| Findings | Implemented resolution |
| --- | --- |
| 1–2 | Viewport-only rows, cached filters, adaptive redraws, coalesced events, bounded streaming metadata for current/visible tracks. |
| 3–5 | Independent background writers, changed-state checkpoints, error retries, versioned cache capped at 3,000 entries with 24-hour expiry. |
| 6–8 | Visible metadata failures and F5 retry, working J reorder, lyrics guarded by real metadata and request generations. |
| 9–11 | Queue epochs reject stale jobs, complete playlist pagination with partial-failure reporting, cancellable connection setup preserving pending controls. |
| 12–13 | Documented cache retention, clear-cache command and account cleanup; synthetic visuals explicitly labeled decorative. |
| 14–18 | Local compiled CSS and deferred script, accessible scoped controls, distinct sample views/lyrics, transform animations suspended while inactive, unsupported performance claims removed. |
| Additional improvements | Wrapped Help/plain lyrics scrolling, system fonts, clipboard fallback, complete themes, queue mutation limits, robust LRC parsing, monotonic playback timing, consistent search/two-login guidance, Windows Rust/browser CI. |

See [VALIDATION.md](VALIDATION.md) for 67 passing offline Rust tests, 11 passing
Edge tests, clean formatting/Clippy, and the rebuilt release executable. See
[BENCHMARKS.md](BENCHMARKS.md) for the measured rendering improvements and
methodology. Live audio, real-terminal output, actual process CPU/RAM, and
Core Web Vitals were not measured again in this implementation pass.

## Validation and measured results

- `cargo test --locked`: **43 passed, 2 ignored**. The ignored tests require live Spotify/audio or a real terminal.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- Four additional probes passed in an isolated temporary source copy: reproduction of the intercepted J shortcut, placeholder metadata passing the lyrics fetch guard, HTTP 503 metadata failures becoming successful empty results, and an optimized render scaling measurement. These probes assert the observed defects; they are not evidence that the defects are fixed.
- A Node VM harness executing the website's actual simulator script with stub DOM elements reproduced four behaviors: Help retains song rows, Space on a focused button is intercepted, a second radio invocation reports six additions despite adding none, and Yellow displays Queen lyrics. This is script validation, not a real-browser rendering test.
- GitHub's latest release API reports v0.2.0 with both `tuitify.exe` and `Tuitify-0.2.0-windows-x86_64.zip`; current download asset names are valid. The executable asset is 12,189,696 bytes. File size does not establish RAM usage.

The render probe used the current source, an optimized release test build, Ratatui's 120×35 TestBackend, a paused Queue view, cached synthetic track metadata, one warm-up frame, and 30 measured draws per size:

| Queue entries | Mean draw time | Approximate core time at 30.3 draws/sec |
| --- | ---: | ---: |
| 50 | 0.181 ms | 0.55% of one core |
| 500 | 0.919 ms | 2.8% of one core |
| 5,000 | 8.563 ms | 26% of one core |

These are local microbenchmarks, not a process CPU measurement or a hardware-independent guarantee. The core-time estimate is draw time divided by the 33 ms tick interval. It excludes physical terminal I/O, metadata scans, persistence, network work, and audio. A paused list growing 100× made drawing approximately 47× slower.

No live account credentials were inspected, no audible playback was started, and no deployment was changed. Browser Core Web Vitals, mobile rendering, actual process RAM, startup latency, and audio-device behavior remain unmeasured in this review.

## TUI performance

### 1. P1 — Redraw frequency and full-list allocation scale poorly

Evidence: `src/app.rs:1000`, `src/app.rs:1019`, `src/ui.rs:861`, `src/ui.rs:885`, `src/ui.rs:1050`, `src/app.rs:136`.

The loop redraws before every select operation and wakes every 33 ms regardless of playback or overlay state. Catalog and queue rendering construct rows for every loaded entry, even though a terminal displays only a small viewport. Ratatui's output diff does not eliminate this row construction. Filtering also repeatedly allocates an index vector and lowercases every title/artist; some render paths compute filtered counts and indices separately.

The measured paused-queue scaling makes this a demonstrated bottleneck rather than a speculative optimization. Multiple incoming metadata events can trigger still more complete draws between animation ticks.

**Fix:** Track dirty UI regions/state, coalesce events before drawing, and use 30 FPS only while visible animation requires it. Render only a viewport plus a small margin, preserve scroll state, and cache filtered indices until the filter or source rows change. Keep nonanimated playback timestamps on a slower update schedule.

**Verify:** Repeat release measurements at 50/500/5,000 entries, including filtered catalog views and a real terminal. Paused draw count should stop growing while the UI is unchanged, and viewport construction cost should not grow proportionally with the entire library.

### 2. P1 — Metadata hydration scans the entire queue and creates one task per missing track

Evidence: `src/app.rs:429`, `src/app.rs:452`, `src/app.rs:1128`, `src/catalog.rs:108`.

Metadata collection starts with the current/nearby entries but then appends the whole queue. When no fetch is running, this path runs approximately ten times per second, cloning and checking every ID even after all names have loaded. On a cold restore, `tracks()` creates one Tokio task per missing ID; its semaphore bounds active requests to five, but does not bound allocated tasks. A 5,000-track cold queue can therefore produce 5,000 individual HTTP requests/tasks. Results are held until the entire collection finishes, delaying even the current track's metadata.

**Fix:** Schedule hydration when the queue or viewport changes. Fetch current and visible entries first, stream results as they arrive, and prefetch a small next window. Use a bounded stream/worker pool instead of spawning every task before acquiring permits. Respect the development-mode endpoint restriction; changing blindly to Spotify's batch endpoint is not a safe fix.

### 3. P1 — Cache persistence blocks the UI and discards errors

Evidence: `src/app.rs:1133`, `src/app.rs:1147`, `src/storage.rs:117`.

Although config/queue persistence uses `spawn_blocking`, `save_cache()` runs directly inside the UI loop. It serializes the full map, flushes it with `sync_all`, and atomically replaces the file. Cache growth or slow storage can stall input and rendering. The result is ignored and `cache_dirty` is cleared even when saving fails, preventing retry unless another cache mutation occurs.

**Fix:** Send cache snapshots/revisions to a background persistence worker. Coalesce updates, acknowledge the saved revision, keep dirty state on failure, and surface a useful error without overwriting it on every frame.

### 4. P2 — Unchanged state is cloned and saved unnecessarily

Evidence: `src/app.rs:983`, `src/app.rs:1022`, `src/app.rs:1132`, `src/storage.rs:80`.

Every key event schedules a full config/queue snapshot, including search typing, navigation, and key releases that the handler ignores. A separate two-second timer does the same while paused and unchanged. Each save rewrites and flushes both files. The watch channel coalesces pending writes, but it does not avoid repeated full queue clones or completed redundant writes.

**Fix:** Track config, queue, selection, and position revisions separately. Debounce selection changes, checkpoint position only while it changes, and save config only when settings change. Preserve the normal-exit flush.

### 5. P2 — Metadata cache limits do not cover normal browsing

Evidence: `src/app.rs:1037`, `src/app.rs:1051`, `src/app.rs:1074`, `src/app.rs:1093`, `src/storage.rs:77`.

Eviction runs only on `Background::Metadata`, not on catalog page, playlist enqueue, or radio insertions. Browsing can therefore grow the map beyond the nominal 3,000-entry threshold and persist it indefinitely. Cached metadata has no expiry, and F5 clears requested IDs but does not invalidate existing cached queue tracks. Names/availability may remain stale. Eviction protects all queue IDs, so a large queue can exceed the limit even on the eviction path.

**Fix:** Centralize insertion and eviction, define separate limits for browse and queue metadata, add version/expiry information, and explicitly invalidate selected/current metadata on refresh. Test browsing beyond the limit, restart behavior, and changed availability.

## TUI reliability and behavior

### 6. P1 — Failed metadata requests are silently treated as success

Evidence: `src/catalog.rs:134`, `src/app.rs:461`, `src/app.rs:1111`.

`tracks()` keeps only `Ok(Ok(track))` results and returns `Ok(tracks)` even if every HTTP request fails. The isolated HTTP 503 probe confirmed `Ok([])`. The application has already inserted all IDs into `requested`, so missing entries stay at “Loading track info...” without the error path clearing that state. Manual F5 can allow another attempt, but the original error and retry instructions never reach the user.

**Fix:** Return per-ID successes/failures, distinguish permanent missing tracks from transient failures, remove retryable IDs from the in-flight set, and expose rate-limit/auth/network state. Stop scheduling more work during cooldown; use bounded retries or explicit user retry.

### 7. P2 — J never reaches the move-down action

Evidence: `src/app.rs:647`, `src/app.rs:932`.

The earlier PageDown/J/> match arm accepts Queue view, making the later Queue-specific J arm unreachable in practice. The isolated key-handler probe confirmed that Shift+J leaves order unchanged and moves selection from 0 to 15.

**Fix:** Put queue reorder handling before paging or remove J from the generic paging arm. Add a key-handler regression test; testing `Queue::move_item` alone cannot catch dispatch conflicts.

### 8. P2 — Lyrics can get stuck after a cold queue restore

Evidence: `src/app.rs:1006`, `src/app.rs:1064`, `src/model.rs:14`, `src/lyrics.rs:65`.

`Track::unknown` uses a nonempty “Track <id>” name. The lyrics guard therefore requests lyrics with that placeholder, no artist, and zero duration, then records the track ID as requested. When real metadata arrives, the ID has not changed, so lyrics are not fetched again. Network failures are also collapsed into “no lyrics,” without a same-track retry path. Fetching runs even while the lyrics overlay is closed, including paused restored tracks.

**Fix:** Fetch only from verified metadata, preferably when lyrics are requested. Key results by track plus metadata revision, distinguish not-found from failure, and provide retry. Clear lyrics state when the queue/current track disappears. Document the metadata sent to Lrclib.

### 9. P2 — Late radio and playlist jobs can modify an unrelated queue

Evidence: `src/app.rs:362`, `src/app.rs:392`, `src/app.rs:1068`, `src/app.rs:1086`.

Start Search playback or Radio, then clear the queue or play a liked-song list before recommendations finish: the response handler ignores `_seed_id` and appends to whichever queue exists at completion. There is no queue revision check. Aborting a previous radio task when starting another radio request does not cover ordinary queue replacements, nor already-delivered messages. Playlist enqueue tasks have no retained cancellation handle either.

**Fix:** Tag jobs with queue/request revisions and an explicit append/replace intent. Cancel or reject stale completions on queue replacement/clear. Test delayed responses after each transition, including two requests for the same seed.

### 10. P2 — “Enqueue entire playlist” only loads its first page

Evidence: `src/app.rs:392`, `src/app.rs:1071`, `src/ui.rs:724`.

The enqueue task calls `page(..., 0)` once and discards `page.next`. A playlist longer than 50 entries is silently truncated despite Help promising the entire playlist. The completion count includes unplayable entries that are subsequently skipped.

**Fix:** Follow pagination with cancellation, progress, and rate-limit handling; report the number actually appended and partial failure. Alternatively explicitly label this action as adding the first page. Bound duplicate jobs from repeated keypresses.

### 11. P2 — Playback connection blocks command processing

Evidence: `src/playback.rs:88`, `src/playback.rs:156`.

The worker awaits `connect()` inside its command branch. While token retrieval and the up-to-35-second session connection are running, Pause/Stop/new Load commands accumulate in the unbounded channel. The UI stays interactive, but the worker cannot honor cancellation promptly, and stale load requests may be processed after the connection completes.

**Fix:** Represent connection as a separately polled/cancellable future, keep handling commands, retain only the latest load generation, and apply volume/pause intent before starting audio. Verify with a delayed mock connector instead of requiring real Spotify outages.

### 12. P2 — Persistent metadata contradicts the privacy documentation

Evidence: `src/storage.rs:77`, `src/storage.rs:84`, `src/main.rs:60`, `src/auth.rs:445`, `README.md:167`.

The README says there is no persistent metadata cache. The app writes `cache.json` containing tracks encountered while browsing and playing; logout and account replacement clear the queue but retain that file. This is retained library/browsing metadata, although it is not a timestamped listening log. It also allows account-dependent availability metadata to carry over between accounts.

**Fix:** Document the cache and its location/retention, provide clear-cache behavior, and clear or partition account-dependent data on logout/account changes. Keep cached credentials out of this file as the current design already does.

### 13. P2 — The advertised spectrum is a synthetic animation

Evidence: `src/ui.rs:164`, `src/ui.rs:588`, `docs/index.html:477`, `README.md:95`.

Both visualizer implementations derive bars from sine/cosine functions of the animation frame and column. Neither consumes audio samples. Frequency labels therefore do not represent the playing audio, and the animation would look the same for silence and music while state is Playing.

**Fix:** Label it as a decorative retro visualizer, or implement actual audio analysis through a bounded sample handoff and background FFT/RMS calculation. Do not add expensive analysis to the audio output callback. If keeping the current animation, calculate each column height once per frame instead of once for every row and column.

## Website

### 14. P1 — Two parser-blocking Tailwind runtimes ship to visitors

Evidence: `docs/index.html:11`, `docs/index.html:12`, `docs/index.html:14`.

The document loads two Tailwind-related scripts synchronously in the head and relies on runtime CSS generation. This adds network/parser dependencies and client-side compilation to a static landing page. There is no compiled stylesheet fallback if those services are unavailable. Having two runtimes also introduces unnecessary overlap and configuration uncertainty.

**Fix:** Build a single version-pinned production CSS file and serve it locally. Include dynamic simulator classes in Tailwind's source discovery/safelist, especially classes assigned from JS. Verify every theme and active-button state after extraction. Measure cold-load LCP and main-thread work before and after; no numeric website speedup is claimed here.

### 15. P2 — Demo keyboard handling breaks normal page controls

Evidence: `docs/index.html:323`, `docs/index.html:393`, `docs/index.html:401`, `docs/index.html:422`, `docs/index.html:980`, `docs/index.html:1094`.

The global listener skips only INPUT. Space on a focused install/demo/FAQ control is prevented and toggles simulated playback instead. Letter shortcuts also run outside the demo. Track rows and seek progress are clickable divs without equivalent native keyboard controls. Inputs lack associated accessible labels, and the play button is identified by a changing symbol rather than a stable accessible action label.

**Fix:** Scope shortcuts to a focusable demo region, respect modifiers and editable/native interactive elements, and allow users to disable single-letter shortcuts. Use buttons for track actions, a labeled range input for seeking, associated labels for filter/volume, and aria-pressed/selected plus meaningful play/pause labels. Verify keyboard-only and screen-reader use.

### 16. P2 — Demo views and content do not match their controls

Evidence: `docs/index.html:915`, `docs/index.html:931`, `docs/index.html:995`, `docs/index.html:1045`.

`switchView` only changes tab styles; Search, Playlists, Liked, Queue, and Help all render the same catalog. All tracks reuse the same Queen lyrics. Repeating Radio adds zero songs but reports six through `added || 6`. `selectAndPlay` does not update `selectedIndex`, so selection styling can remain on the first row.

**Fix:** Give each supported demo view a real state model, or reduce controls to the interactions the demo actually implements. Use track-specific sample lyrics or a clear unavailable state, report actual additions, and keep selection consistent. State explicitly that the demo simulates playback.

### 17. P2 — Continuous layout animation and avoidable DOM updates

Evidence: `docs/index.html:93`, `docs/index.html:931`, `docs/index.html:1128`.

Bar animation changes height, which requires layout, and remains active while simulated playback is paused. There is no reduced-motion handling. A permanent timer updates the demo regardless of whether it is onscreen; active lyrics rebuild their HTML every second even when the highlighted line has not changed.

**Fix:** Use transform: scaleY with a bottom transform origin, pause animation when paused/offscreen, respect prefers-reduced-motion, and suspend timer work for hidden pages. Update lyrics only when the active index changes and use monotonic elapsed time if the simulated clock needs to remain accurate across throttling.

### 18. P2 — Performance claims lack reproducible evidence

Evidence: `docs/index.html:535`, `docs/index.html:568`, `docs/index.html:574`, `docs/index.html:580`, `VALIDATION.md`.

The site advertises ~14 MB RAM, 12 ms startup, <0.1% idle CPU, <1 ms filtering, and precise competitor comparisons. The repository has no benchmark harness/results explaining workload, hardware, memory metric, sample count, or versions. The validation record documents functional acceptance, not those numbers. Executable size is also mixed into the RAM discussion. The render probe demonstrates why queue size and workload must be specified for idle CPU claims; it does not establish process RAM or startup time.

**Fix:** Remove or qualify precise claims until measured. Publish an automated benchmark procedure with cold/warm startup definitions, idle/playing/visualizer workloads, queue sizes, working set versus private bytes, CPU sampling duration, versions, and repeated measurements. Compare competing clients under equivalent workloads.

## Additional improvements

- **P2, Help scrolling:** `src/app.rs:190` uses a fixed length while `src/ui.rs:752` caps scroll at 30 despite a longer help document and wrapping. At small heights/widths the last instructions cannot reliably be reached. Derive bounds from rendered content and viewport height; test the final troubleshooting line at 32×10.
- **P3, Font loading:** `docs/index.html:35` uses CSS @import for two font families with many weights. Prefer local subset WOFF2 or explicit stylesheet links/preconnects, and load only used weights.
- **P3, Clipboard feedback:** `docs/index.html:1086` handles only success. Add rejection/unavailable handling and selectable command text so copying still works when clipboard access is denied.
- **P3, Theme completeness:** `src/ui.rs:184` and catalog/queue rows use the Spotify block/default GREEN constants. Route all surfaces through the selected theme for consistent results.
- **P2, Queue storage limit:** `src/storage.rs:130` rejects >100,000 IDs on restore, while enqueue paths do not enforce that boundary. Apply the same invariant when mutating the queue so the app cannot save a snapshot it subsequently refuses to open.
- **P3, Lyrics parsing/timing:** `src/lyrics.rs:39` supports only the first timestamp per line; arithmetic is unchecked, and current-line lookup highlights the first line before its timestamp. Add checked parsing, multiple timestamp support, pre-first-line state, and scrolling for long plain lyrics. In `src/app.rs:1124`, interpolate position from a monotonic playback timestamp instead of adding a fixed 33 ms after skipped/delayed ticks.
- **P2, Documentation consistency:** Search playback now replaces the queue with a single seed plus recommendations (`src/app.rs:712`), whereas README describes loaded-list replacement. Document the actual behavior or make auto-radio a preference. Explain the two-login flow directly in website setup guidance as well.
- **P2, CI coverage:** The release script is useful, but no CI workflow is present. Run the existing Rust checks on Windows and add targeted key-handler, delayed-background-response, metadata-failure, and browser keyboard tests. Current one-track rendering tests cannot detect large-list performance regressions.

## Recommended implementation order

1. Fix metadata error propagation, queue revision checks, J dispatch, and lyrics readiness. Add targeted regression tests.
2. Implement viewport rendering, adaptive redraws, bounded metadata hydration, and background/revision-based persistence. Rerun the release microbenchmark and measure actual process CPU/RAM.
3. Compile website CSS, repair keyboard accessibility and demo semantics, and make animations visibility-aware.
4. Align privacy/feature documentation with behavior, then replace unsupported benchmark claims with published measurements.

Keep the existing strengths: atomic queue/config replacement, the instance lock, PKCE/state validation, serialized token refresh, playback generation checks, and the offline test suite. The main work is to carry those same correctness boundaries into newer cache, lyrics, radio, and demo features.
