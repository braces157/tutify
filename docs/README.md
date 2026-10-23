# Tuitify documentation

This page is the entry point for everything outside the main
[README](../README.md). Read the column labels before relying on a number:
Tuitify's older review documents are deliberately kept as historical evidence,
and their counts describe the snapshot they were written against.

## Start here

| Guide | What it answers |
| --- | --- |
| [README](../README.md) | What Tuitify is, how to install it, every control, and how playback works |
| [Volta setup](volta.md) | Create a personal Volta application, grant permissions, and play lossless audio |
| [Architecture](../ARCHITECTURE.md) | Which module owns what, and the boundaries to preserve when changing code |
| [Changelog](../CHANGELOG.md) | What changed in each release and what is in the working tree |
| [Troubleshooting](../README.md#troubleshooting) | Doctor, support reports, F7 errors, and common failures |

## Reference and engineering

| Document | Status | Contents |
| --- | --- | --- |
| [ARCHITECTURE.md](../ARCHITECTURE.md) | Current | Module responsibilities, state ownership, runtime and validation rules |
| [BENCHMARKS.md](../BENCHMARKS.md) | Current | How to reproduce rendering, memory, and persistence measurements |
| [VALIDATION.md](../VALIDATION.md) | Current, dated | Verification performed for each release, including what was *not* verified |
| [CHANGELOG.md](../CHANGELOG.md) | Current | Per-release change history, newest first |
| [ROADMAP.md](../ROADMAP.md) | Current | Deferred work, release history, and explicit non-goals |
| [docs/DESIGN.md](DESIGN.md) | Current | Design system and tokens for the static website in `docs/` |
| [Release notes](releases/v0.7.0.md) | Current per release | Short, user-facing notes; see the other files in `docs/releases/` for older versions |

## Validation records by feature

These record a specific investigation, its environment, and its limits. They are
useful for reproducing a check, not as a live status report.

| Record | Topic |
| --- | --- |
| [monochrome-playback-validation.md](monochrome-playback-validation.md) | Monochrome matching, FLAC header parsing, and the first audio attempt |
| [be-a-flower-playback-validation.md](be-a-flower-playback-validation.md) | Full-track YouTube continuity, interrupted HTTP streams, and bilingual titles |
| [deezer-recommendation-validation.md](deezer-recommendation-validation.md) | Related-artist balancing for Radio, Smart Shuffle, Autoplay, and Mix Builder |
| [personalized-recommendation-validation.md](personalized-recommendation-validation.md) | Local taste signals, thresholds, decay, and saved `stats.json` feedback |
| [smart-shuffle-validation.md](smart-shuffle-validation.md) | Earlier Spotify-era discovery rules and live acceptance seeds |
| [listening-tools-validation.md](listening-tools-validation.md) | Sleep timers, queue cleanup, Play Next, and search recall |
| [queue-filter-validation.md](queue-filter-validation.md) | Queue filtering, original row positions, and undo behavior |

## Historical snapshots

These documents describe an earlier state of the project and are kept for
provenance. Do not read their counts or line references as current.

| Document | Snapshot |
| --- | --- |
| [PERFORMANCE_REVIEW.md](../PERFORMANCE_REVIEW.md) | 2026-09-06 review; the findings were subsequently implemented, see its follow-up table |
| [REFACTOR_REVIEW.md](../REFACTOR_REVIEW.md) | 2026-09-08 pre-refactor review; the refactor has since been applied |

## Local-only planning notes

`AGENTS.md`, `PROJECT.md`, `TEST_INFRA.md`, `TEST_READY.md`, `FUTURE_PLAN.md`,
`IMPLEMENTATION_PROGRESS.md`, and `ORIGINAL_REQUEST.md` are excluded by
`.gitignore`. They are development notes rather than published documentation, so
a fresh clone will not contain them and release archives do not include them.
Where they remain in a working copy, their status is:

- `PROJECT.md`, `TEST_INFRA.md`, and `TEST_READY.md` describe the v0.2.8 album and
  artist browsing milestone. Their test counts (193 baseline, 264 total) are a
  dated snapshot; the suite has since grown well beyond them.
- `FUTURE_PLAN.md` and `IMPLEMENTATION_PROGRESS.md` track the numbered task
  checklist and its delivery evidence.
- `ORIGINAL_REQUEST.md` preserves the original task text and acceptance criteria.

`docs/website-design.md` is also excluded from version control (through
`.git/info/exclude`), but because release packaging copies the whole `docs/`
directory it does travel inside the ZIP. It records the website redesign rounds
and their review scores, and the current design reference is [DESIGN.md](DESIGN.md).

## Keeping documentation accurate

- Facts belong in one place. The README owns user-facing behavior; release notes
  own what shipped; `VALIDATION.md` owns what was executed and what was not.
- Prefer a dated heading over an undated claim, and state limits next to results.
  "Not verified" is a valid and useful outcome.
- When a number changes (test counts, limits, timeouts), update the owning
  document and any summary that quotes it.
- Local links are checked during release packaging, so a moved file must update
  every link to it. Run the packaging check or a link scan before publishing.
