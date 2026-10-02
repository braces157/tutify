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
| `src/catalog/similarity.rs`, `discovery.rs` | Related-artist identity checks, bounded candidate pages, original-seed discovery |
| `src/catalog/capabilities.rs` | Bounded client/account session observations, per-resource denial expiry, refresh and identical-probe serialization |
| `src/service.rs` | Classified, redacted service failures and retry deadlines |
| `src/diagnostics/doctor.rs`, `doctor/installation.rs`, `src/auth/doctor.rs` | Read-only local diagnostics, saved/process PATH hashes, and explicitly opted-in GET probes without credential refresh |
| `src/diagnostics/history.rs`, `support.rs`, `src/app/diagnostics.rs`, `src/ui/diagnostics.rs` | Bounded typed session errors, redacted report snapshots, preview/export controls and F7 rendering |
| `src/playback.rs`, `visualizer.rs` | Streaming engine, audio output and spectrum analysis |
| `src/queue.rs`, `stats.rs`, `cache.rs`, `storage.rs` | Domain data and persistence |
| `src/storage/backup.rs` | Versioned saved-state backup, preview confirmation, staged restore and journal recovery |
| `src/model.rs` | Shared track/repeat/playback-state types |
| `src/mix.rs`, `src/ui/mix_builder.rs` | Deterministic Mix Builder domain model and overlay |
| `src/media_controls.rs`, `discord.rs` | Windows media controls and Discord presence |

## Boundaries to preserve

- The error journal belongs to `UiState`, holds at most 64 typed records, and is
  separate from the transient status string and every persistence snapshot.
  Record current-generation background/playback/storage failures at application
  boundaries; stale background results remain ignored. Typed service errors retain
  provider/kind/status/minimum retry wait; compatibility strings retain only
  classified static phrases, never their original contents.
  Support reports serialize allowlisted build/check/capability/error fields rather
  than attempting to redact arbitrary strings. Doctor detail/action text, names,
  paths, identifiers, searches, queue and listening data are never copied into
  reports. Catalog summaries contain only endpoint labels and counts of current
  observations; pending/expired slots are unknown, and snapshots cannot block on
  in-flight async probes. F7 report previews are immutable snapshots; explicit export
  publishes the reviewed bytes outside saved data with non-clobbering publication.
  The standalone offline support command has its own empty session journal and
  does not recover another process's errors. No report is uploaded automatically.

- Doctor branches before storage creation, instance locking, journal recovery,
  terminal entry, and auth setup. It inspects state through the existing strict
  version/invariant readers, reads credentials without publishing changes, and
  queries CPAL devices/configurations without creating an output stream. Its
  credential manager disables persistence and refresh (including rejected-token
  refresh), so network diagnostics make only catalog GETs. Probe timeouts include
  body decoding; systemic failures skip remaining probes. Resource capabilities
  remain account/resource/session observations, and untested writes stay unknown.
  Native registry reads compare saved Machine/User PATH separately from process
  PATH. SHA-256 comparisons detect copies with the same version but different
  bytes. Local JSON contains paths and is not a shareable redacted support report.

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
- Library traversal skips only classified playlist-items restrictions/missing
  items carrying catalog-denial provenance. Token-service errors, throttle gates,
  outages, and invalid responses stop traversal. Progress streams skipped-source
  deltas; navigation retains the source list through `Arc`. Finishing traversal
  does not imply complete coverage when any source was skipped. F4 reads the list
  independently of transient status; F5 starts a new scan with fresh access probes.
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
- Credential identity metadata has its own version and distinguishes immutable
  Web API account IDs, legacy user IDs, and streaming usernames. The old token
  `account_id` remains a compatibility alias with role-specific interpretation.
  Stable-ID conflicts cannot be overridden by alias equality. Alias migration
  can verify the previous credential with its original client without persisting
  refresh changes. A streaming bridge uses a fresh catalog legacy handle matching
  the authenticated AP username, a previously verified mapping with a fresh stable
  profile, or matching /me profiles for catalog and streaming tokens. Stable IDs
  are never compared to usernames. Catalog tokens lack streaming scope and are
  never used for AP authentication. Playback checks its authenticated username
  before constructing the audio player. Reauthentication never resets account
  files: ambiguous/conflicting identity requires recovery or deliberate logout.
  Unknown credential schemas and damaged credentials are preserved. Config-write
  failures restore the previous catalog credential; streaming-write failures
  restore the prior catalog metadata. Rollback failures are reported explicitly;
  this is retryable multi-store coordination, not a power-loss atomic transaction.
- Catalog capabilities belong to an immutable client/account session and its
  clones. A bare item denial never establishes a global endpoint removal. Only
  a denial from the requested catalog endpoint may populate capability state or
  trigger fallback; OAuth failures cannot. Support requires the expected response
  shape. Expired observations are unknown, and F5/Mix Retry detaches old slots
  without clearing quota/rate gates. Different resources remain independent.
  Empty successful recommendation pools stay with their actual provider.
- Artist pages carry typed result provenance from catalog/demo through jobs and
  navigation snapshots. Artist Search continuations retain their source even when
  access observations expire or are refreshed. Offset-zero refresh can establish a
  new source and replaces the previous rows; pending/error refreshes preserve the
  provenance of retained rows. Search row positions are not Top Tracks rankings.
- Keep disk writes off the event loop. Writers coalesce snapshots, report failure,
  retry on later checkpoints, and flush their final value on shutdown. Preserve
  atomic JSON replacement and existing persisted formats. Checkpoints share
  immutable snapshots through `Arc`; buffered JSON is flushed before file sync.
  Unreadable or unsupported statistics/recipes stop startup before workers begin.
- Backup/restore runs under the instance lock before authentication or playback.
  It includes only config, queue, recipes, and aggregate stats, with explicit saved
  or missing snapshots and per-file validation. Credentials/cache are not read or
  restored. Confirmation binds the backup bytes, canonical destination, and current
  file bytes/presence. All replacements are staged and synced before publishing.
  The restore journal records original bytes for exact rollback, including invalid
  old state. Locked startup recovers an uncommitted journal or cleans up a committed
  one before loading state; unexpected edits or malformed journals stay preserved.
- Per-file recovery is whitelisted to config, queue, recipes, stats, and cache.
  Inspection and startup reads classify file, version, JSON/schema, and invariant
  failures without echoing raw JSON or user-supplied field values. Component
  backups preserve exact bytes even for invalid/future schemas. Targeted restore
  accepts a checked component envelope or one whole-backup snapshot and validates
  incoming state; unrelated current files are never loaded or replaced. Preview
  confirmation binds selected bytes, root, operation, source, and archive path.
  Applying first publishes a non-clobbering external original-file backup, then
  uses the shared staged/journaled restore transaction for one file, including
  cache. Ordinary writers and whole-state restore cannot overwrite recognizable
  future versions. Explicit targeted recovery preserves those originals externally.
- Statistics cache normalized titles once, filter borrowed rows, and sort once.
  All-time totals remain independent of the filter. Metadata hydration updates
  only the corresponding saved statistics entry.
- Discovery prefers exact artist names. Latin accent aliases require a matching
  seed recording; artist IDs reject ambiguous and unrelated candidate pools.
  Lyrics retain Unicode accents and canonicalize equivalent Unicode sequences;
  a missing full credit can retry a verified lead artist with title/duration checks.
  Lyrics service retries are bounded, and failure remains distinct from missing data.
- Mouse regions follow the renderer's actual wrapping and clipping. Statistics
  searches and recipe names reserve room for real footer actions. Applying a mix
  remains an explicit keyboard operation.
- FFT bins use the sample rate published by the Windows output device. Runtime
  frame deadlines use the last draw time; remaining debounce/fade delays use now.
- The terminal guard, Windows media session, Discord task, playback worker and
  background jobs have explicit cleanup. Preserve cleanup ordering when changing
  startup or exit paths. Shared playback state lives in `model`, so integrations
  do not import the application controller.

## Validation

Run `cargo fmt --check`, `cargo test --locked`, and
`cargo clippy --all-targets --locked -- -D warnings` for Rust changes. Build with
`cargo build --release --locked` before release. Tests live beside their modules;
`src/app/tests/` retains interaction tests across input, state, jobs and persistence.

The sixteen opt-in checks cover live catalogs/lyrics/streaming, a silent Windows
media session, real-terminal cleanup, and optimized rendering, persistence,
statistics, and Mix Builder benchmarks. List them with `cargo test -- --list` and
run the relevant checks when touching those paths. Run `npm test` for
website changes. See `VALIDATION.md` for what was actually executed.

Prefer cohesive modules of a few hundred lines, but do not split working domain
modules solely to meet a numeric limit. New abstractions should own an invariant
or remove duplicated behavior; moving fields alone is insufficient.
