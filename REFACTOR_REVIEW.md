# Codebase refactoring review — 2026-09-08

Implementation follow-up: the staged refactor has now been applied. The findings
and line references below describe the pre-refactor snapshot. See
[ARCHITECTURE.md](ARCHITECTURE.md) for the resulting module boundaries and
[VALIDATION.md](VALIDATION.md) for implementation verification. Application and UI
roots are now 589 and 151 lines respectively; App has 24 top-level fields.

**Recommendation: refactor the application controller and TUI incrementally before adding more interaction modes. Keep the existing domain modules and runtime architecture.** The main problem is shared state and scattered behavior, rather than a repository-wide excess of lines.

The original review covered the working tree based on `35c5332`, including the existing uncommitted changes in README, app, catalog, stats, and UI. It was an architecture and maintainability review, with targeted inspection of correctness boundaries; it was not a comprehensive security or performance audit. Application code was changed only in the subsequent implementation described above.

## Measured size

Counts include comments and blank lines. “Before tests” means the portion before the trailing `#[cfg(test)] mod tests`; it is an approximation of production size and includes occasional test-only helpers. Generated CSS, assets, dependencies, and build output are excluded.

| Rust file | Total lines | Before tests | Recommendation |
| --- | ---: | ---: | --- |
| app.rs | 4,054 | 2,499 | Highest priority: separate responsibilities and state ownership |
| ui.rs | 2,560 | 2,016 | High priority: split rendering by panel and expose render state |
| auth.rs | 1,089 | 787 | Later: separate login workflow from token lifecycle |
| discord.rs | 839 | 474 | Keep cohesive; optional protocol/activity split later |
| stats.rs | 831 | 434 | Separate presentation state when restructuring App |
| catalog.rs | 728 | 418 | Keep; recommendations may eventually merit a child module |
| visualizer.rs | 717 | 568 | Optional DSP/source/presentation separation; preserve audio path |
| playback.rs | 639 | 427 | Keep engine boundary; improve dense event handling when touched |
| library.rs | 404 | 243 | Keep |
| media_controls.rs | 344 | 190 | Keep; remove dependency on app-owned playback state |
| storage.rs | 322 | 171 | Keep atomic persistence implementation |
| queue.rs | 309 | 205 | Keep; improve invariant ownership incrementally |
| lyrics.rs | 161 | 123 | Keep |
| cache.rs | 150 | 125 | Keep |
| main.rs | 96 | 96 | Keep thin entry point |
| model.rs | 72 | 72 | Suitable home for shared playback state |
| diagnostics.rs | 48 | 48 | Keep |
| **Total** | **13,363** | **8,896** | |

Only **three of 17 Rust files exceed 1,000 lines**. Approximately 33% of Rust lines are in the trailing test sections. App and UI account for about **51% of the pre-test code**. The website's `demo.js` is 637 lines and `index.html` is 725; neither warrants a framework migration based on size.

## Findings, ordered by value of refactoring

### 1. App owns too many independent state machines

Evidence: `src/app.rs:117` defines 57 fields, 48 public. These mix queue undo, playback/accounting, catalog browsing, text input, radio, lyrics, overlays, rendering geometry, and persistence inputs. Overlay state uses three separate booleans. Navigation and overlay transitions repeatedly reset combinations of flags (`app.rs:1074`, `1254`, `1884`). `selected` also serves several views, while queue selection is stored separately.

The cost is that a feature must understand unrelated modes to avoid changing their behavior. Existing regression tests at `app.rs:3794`, `3821`, `3854`, `3903`, and `4016` explicitly protect accounting and selection/shortcut isolation. They demonstrate the importance of these interactions; their presence does not imply those regressions currently fail.

**Change:** introduce focused `BrowseState`, `LyricsState`, and `UiState` incrementally. Represent mutually exclusive overlays with `Overlay::{None, Lyrics, Visualizer, Stats}`. Keep focus/text-entry state separate where it is genuinely orthogonal. Give stats its own selection alongside its existing query/sort state. Centralize transitions such as opening an overlay and changing views.

Do not simply move the same public fields into many structs and let every module mutate all of them. The benefit comes from a small set of transition methods enforcing invariants.

### 2. Input routing and application actions are entangled and duplicated

Evidence: `key` spans `app.rs:1367–2021`, roughly 655 lines. It handles mode precedence, editing, navigation, queue replacement, seeking, volume, settings, and network requests. Seek/volume/mute logic appears in both stats mode (`1433–1512`) and normal mode (`1793` onward). Mouse controls and context menus synthesize keyboard events (`1158`, `1334`), whereas media keys already use a semantic `media_action` method (`581`).

This makes a shortcut a de facto application API. A future change to input precedence can change what a context-menu action does. Duplicated controls also need changes in multiple places.

**Change:** extend the existing semantic-action pattern with seek, volume, mute, selected-track, queue, and navigation actions. Decode keyboard/mouse/menu input into actions, then apply actions through shared methods. Keep mode-specific permissions in the input router: a common executor must not accidentally enable destructive shortcuts in the stats overlay or text entry. Preserve the existing stale-menu revision check before executing an action.

Start with shared seek/volume/mute helpers and small mode handlers; a generic command framework or a full reducer/effects rewrite is unnecessary.

### 3. Rendering has hidden effects that drive application behavior

Evidence: `ui::draw(&App)` clears hit regions and sets terminal dimensions (`ui.rs:205`). `viewport` mutates offsets (`990`); stats rendering refreshes a cached view (`1025`); queue rendering writes visible height and scroll (`1685`). The event loop reads these values to decide metadata loading (`app.rs:2402` onward), and `Tasks::metadata` uses the viewport to bound requests (`998`).

These mutations are intentional optimizations, not inherently erroneous. However, the `&App` signature hides the fact that drawing prepares input routing and network scheduling. Tests of those paths need realistic layout preparation, and splitting panel functions alone will leave the coupling intact.

**Change:** group hit regions, scroll positions, dimensions, and wrapped-content lengths into explicit render/UI state. Initially pass that state mutably while keeping domain data borrowed. Later return compact layout feedback where it simplifies the event loop. Move stats/filter cache refresh into an explicit preparation step when practical.

Preserve visible-row rendering, cached filters, draw throttling, hit-region invalidation, and metadata prefetch limits. Do not clone the full App or queue into a render snapshot every frame.

### 4. Background job state and lifecycle rules are distributed

Evidence: `Background` and `Tasks` (`app.rs:703`, `715`) contain five task handles and several counters/flags. Task creation and cancellation occupy `750–1109`; response application is in `background` at `2023`; run-loop scheduling starts at `2314`. Lyrics request state lives partly in App, while metadata and playlist state live partly in Tasks. Persistence writers and checkpoints are also embedded in the controller (`2232`, `2280`).

Cancellation alone cannot reject a result already queued for delivery. The existing request IDs, queue epochs, and generation checks are therefore important architecture, not expendable bookkeeping.

**Change:** first extract jobs and persistence as private app child modules without changing semantics. Then group each job's handle, identity, and loading/error state where useful. Keep a small coordinator for cross-feature queue transitions. Distinguish queue epoch, playback generation, and individual request identity; consider newtypes if mix-ups remain easy.

Preserve task abortion on drop, partial playlist/library results, stale-response rejection, coalescing writers, retry signaling, and final flush/error propagation. Avoid replacing all job types with one generic task manager: their lifecycles differ.

### 5. UI behavior is inferred from human-readable error text

Evidence: `ui.rs:380` and `1675` both classify authentication state using `app.status.contains(...)`. The first branch displays an “AUTH EXPIRED” banner, although one trigger is merely text containing `auth --force`. Normal actions also overwrite the same status string.

This is a concrete design defect: display wording doubles as a state protocol. Changing an error message can change the banner without any compiler feedback, and a generic reauthorization suggestion is not proof that a token expired.

**Change:** retain separate typed service health/error categories for catalog and playback, plus a transient status message. Rendering should select recovery guidance from the category, while preserving detailed error text for the user. Classification belongs near the operation that knows the failure, not in substring matching inside renderers. No need to replace every `anyhow::Error` in the application.

### 6. Small reverse dependencies blur otherwise useful boundaries

Evidence: both `discord.rs:2` and `media_controls.rs:2` import `app::State`, while App coordinates those integrations. `queue.rs:1` imports storage's `validate_ids`, while storage imports Queue. Catalog imports generic HTTP construction and Retry-After parsing from auth (`catalog.rs:2`). Stats includes both persisted/accounting data and presentation state (`stats.rs:41`).

These are legal Rust module dependencies, but lower-level code should not need the application controller to name playback state.

**Change:** move shared `PlaybackState` into model (or a small shared playback-types module). Let queue validation own its ID/limit checks. Move `StatsView` into UI state during that extraction. Move generic HTTP helpers only if doing so reduces actual coupling; preserve each service's timeout and retry policy. These are small follow-ups, not reasons to introduce a multi-crate workspace.

### 7. UI.rs has clear extraction seams, but some functions also need decomposition

Evidence: `draw` is approximately 374 lines, `center` 435, stats rendering 213, queue rendering 181, and playback rendering 179. Terminal setup/cleanup and theme definitions are in the same file as all panels. `center` handles overlay selection, Help content, search controls, filtering, and catalog rows.

**Change:** extract terminal lifecycle, theme/shared widgets, and individual panels; then make `draw` and center dispatch read as layout composition. Keep panel-specific styling local, sharing only repeated primitives. Avoid a generic widget abstraction that requires more parameters than the concrete panels.

## Proposed module destinations

This is a direction for staged changes, not a requirement to create every file at once. Keep `app.rs` and `ui.rs` as facade/root modules with children under their matching directories.

| Area | Suggested destination | Responsibility |
| --- | --- | --- |
| Application composition | `src/app.rs`, `src/app/runtime.rs` | App facade, startup, event loop, shutdown |
| Input | `src/app/input.rs` | Mode routing and input-to-action mapping; split keyboard/mouse only if still large |
| Actions | `src/app/actions.rs` | Shared playback/queue/view transitions |
| Job coordination | `src/app/jobs.rs` | Background messages, task ownership, result handling |
| Persistence coordination | `src/app/persistence.rs` | Checkpoints, writer lifecycle, save failures |
| Presentation state | `src/app/ui_state.rs` | Overlay, focus, selection, render feedback, stats presentation |
| UI composition | `src/ui.rs` | Layout and panel dispatch |
| UI support | `src/ui/terminal.rs`, `theme.rs`, `widgets.rs` | Terminal guard, colors, repeated rendering primitives |
| Panels | `src/ui/catalog.rs`, `queue.rs`, `playback.rs`, `lyrics.rs`, `stats.rs`, `visualizer.rs`, `help.rs` | Individual panel rendering |

Use private modules and narrowly scoped `pub(super)`/`pub(in crate::app)` visibility where needed. Do not make everything `pub` to get extraction compiling. Existing root-module tests can remain initially; extracted unit tests should live beside the owning module, with interaction tests kept at the App boundary.

## Implementation sequence and acceptance

1. **Mechanical extraction:** separate UI panels/terminal/theme, then app persistence and job coordination. Preserve signatures and behavior. Move large test sections into child test modules for navigation without deleting or weakening them. This reduces review friction but does not by itself complete the architectural work.
2. **Shared actions and input routing:** deduplicate seek/volume/mute, replace menu key synthesis with semantic actions, and split mode handlers. Verify emitted playback commands and mode isolation using existing tests; add only missing cross-input equivalence cases.
3. **State ownership:** introduce overlay state, mode-specific selection, explicit render state, and typed error categories. Re-run realistic draw → input → draw scenarios, especially filtered/scrolled rows and stats overlays. Preserve current behavior unless an intentional behavior change is separately reviewed.
4. **Boundary cleanup:** move playback state and queue validation to their owners. Reassess auth, DSP, and website extraction based on ongoing development needs rather than line thresholds.

After each meaningful stage: run `cargo fmt --check`, `cargo test --locked`, and `cargo clippy --all-targets --locked -- -D warnings`. At completion, run the release build used by CI. Run browser tests if website files change. Run opt-in terminal/media/audio acceptance when touching the relevant lifecycle or integration paths.

Keep changes independently reviewable. Establish the intended starting snapshot of the existing uncommitted work before implementation so review can distinguish feature changes from code movement. No need for a broad rewrite, new framework, dependency-injection container, or mandatory file-size gate. A few hundred production lines per focused module is useful guidance; responsibility and ownership are the acceptance criteria.

## Verification performed for this review

- `cargo test --locked`: **137 passed, 0 failed, 4 ignored**.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --locked -- -D warnings`: passed.
- Inspected the current module inventory, major state/action/task/render paths, relevant regression tests, website structure, CI, and historical validation records.

The four ignored checks cover live streaming, Windows media-session acceptance, real-terminal cleanup, and a release rendering benchmark. They were not run. Browser tests, a release build, and new performance measurements were not run for this read-only application review. Passing tests establish a useful baseline, not exhaustive behavior coverage. Historical validation results are not presented as fresh verification.
