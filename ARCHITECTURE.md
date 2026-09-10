# Source architecture

Tuitify is a single Rust binary with a Tokio application loop, an independent
streaming worker, and a Ratatui interface. Keep the modules in this crate until
there is a concrete need for a reusable library. The static website under `docs/`
is a separate demonstration, not the player's frontend.

## Where changes belong

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI dispatch and startup authentication |
| `src/app.rs` | Application state, playback transitions/accounting, queue undo |
| `src/app/runtime.rs` | Startup, event scheduling, redraws, shutdown |
| `src/app/demo_runtime.rs`, `src/demo.rs` | Credential-free runtime adapter and bundled fictional catalog |
| `src/app/input.rs` | Keyboard modes and shortcut permissions |
| `src/app/mouse.rs` | Hit testing, context menus, mouse-to-action routing |
| `src/app/actions.rs` | Selected-track and queue actions shared by input sources |
| `src/app/controls.rs` | Shared seek, volume, mute and playback controls |
| `src/app/browsing.rs` | Browse/search state, row revisions, cached filtering |
| `src/app/lyrics_state.rs` | Lyrics data and request identity |
| `src/app/ui_state.rs` | Mutually exclusive overlays, stats presentation/cursor, layout feedback |
| `src/app/jobs.rs` | Background task lifecycle and guarded response application |
| `src/app/persistence.rs` | Changed-state checkpoints and coalescing snapshot writers |
| `src/ui.rs` | Frame composition and explicit layout feedback |
| `src/ui/` panels | Catalog, queue, playback, lyrics, stats, visualizer and Help rendering |
| `src/ui/chrome.rs`, `navigation.rs` | Header/status/menu rendering and responsive navigation |
| `src/ui/terminal.rs`, `theme.rs`, `widgets.rs` | Terminal lifecycle, colors and shared drawing helpers |
| `src/auth.rs` | PKCE, credentials and serialized token refresh |
| `src/catalog.rs`, `library.rs`, `lyrics.rs` | Remote catalog, saved-library traversal and lyrics parsing/fetching |
| `src/playback.rs`, `visualizer.rs` | Streaming engine, audio output and spectrum analysis |
| `src/queue.rs`, `stats.rs`, `cache.rs`, `storage.rs` | Domain data and persistence |
| `src/model.rs` | Shared track/repeat/playback-state types |
| `src/mix.rs`, `src/ui/mix_builder.rs` | Deterministic Mix Builder domain model and overlay |
| `src/media_controls.rs`, `discord.rs` | Windows media controls and Discord presence |

## Boundaries to preserve

- Input routers decide which commands a mode permits. Shared actions execute
  intent without synthesizing keyboard events. New playback shortcuts should use
  the existing control executor; avoid duplicating seek or volume logic in an
  overlay. Context-menu actions validate the captured list revision before use.
- `Overlay` makes lyrics, visualizer, stats, and Mix Builder mutually exclusive. Statistics
  owns its cursor independently of catalog and queue selection. Its query, sort,
  and cached rows are presentation data, not part of the saved statistics schema.
- `BrowseState` owns filter-cache invalidation. Use its row update methods for
  page/progress application. A row mutation must invalidate cached indices even
  when the number of rows stays the same.
- Rendering receives `&App` and a separate `&mut RenderState`. Layout feedback
  includes visible queue height, offsets, hit regions, and wrapped-content limits.
  The runtime consumes it after drawing to schedule bounded metadata requests.
  Render functions must not borrow `app.ui.render` again while that mutable borrow
  is active. Stats/filter caches remain lazily refreshed; avoid cloning the queue
  or full application to prepare a frame.
- Cancellation and stale-response checks serve different purposes. Preserve
  request IDs, queue epochs, and playback generations when changing jobs. A
  cancelled job may already have queued a result. Library/playlist failures retain
  usable partial results.
- Mix source paging and recommendations have a request identity independent of
  the live queue epoch. They may update only the current preview; applying a mix
  is the sole boundary that snapshots and mutates the queue. Recommendation
  provenance contains the actual seed and provider, never inferred rationale.
- Demo mode is selected before production storage or authentication is opened.
  Its runtime supplies fictional catalog pages, lyrics, recommendation outcomes,
  and playback events through the production App/input/UI boundaries.
- Playback accounting is pinned to the loaded generation and its original track.
  Queue replacement, undo, completion, pause, and shutdown must finalize the old
  generation at the existing transition points. Seeking must not add listened time.
- Catalog authentication health comes from typed failures and is shared across
  catalog clones. A volume/status message cannot clear it. Successful catalog
  access clears it; an unrelated network failure does not. Renderers must not
  infer service state by searching human-readable status text.
- Keep disk writes off the event loop. Writers coalesce snapshots, report failure,
  retry on later checkpoints, and flush their final value on shutdown. Preserve
  atomic JSON replacement and existing persisted formats.
- The terminal guard, Windows media session, Discord task, playback worker and
  background jobs have explicit cleanup. Preserve cleanup ordering when changing
  startup or exit paths. Shared playback state lives in `model`, so integrations
  do not import the application controller.

## Validation

Run `cargo fmt --check`, `cargo test --locked`, and
`cargo clippy --all-targets --locked -- -D warnings` for Rust changes. Build with
`cargo build --release --locked` before release. Tests live beside their modules;
`src/app/tests/` retains interaction tests across input, state, jobs and persistence.

The five opt-in checks require specific environments or optimized builds: live
streaming, a silent Windows media session, a real terminal for cleanup, an
optimized rendering benchmark, and the optimized Mix Builder generation
benchmark. Run the relevant checks when touching those paths. Run `npm test` for
website changes. See `VALIDATION.md` for what was actually executed.

Prefer cohesive modules of a few hundred lines, but do not split working domain
modules solely to meet a numeric limit. New abstractions should own an invariant
or remove duplicated behavior; moving fields alone is insufficient.
