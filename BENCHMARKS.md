# Reproducing performance measurements

Tuitify does not promise a fixed RAM footprint, startup time, or CPU percentage.
Those depend on the terminal, hardware, audio device, network, cache, and library.
Executable size is not process memory usage.

## Automatic music launch and idle helper memory — 2026-10-07

Real Windows ConPTY sessions compared the prior executable with the final
refactor executable before its version bump to 0.5.0. Both used a 120x32 terminal
and the same Glass appearance. Each ran public searches in isolated app data
with installed tools, Discord disabled and volume zero. Memory was sampled every
100 ms for Tuitify and its owned helpers, including their consoles, excluding
the external terminal and benchmark Python. Summed working sets can count shared
pages twice; they are not total system-memory usage. Settled idle is the median
of the final ten samples after waiting 35 seconds following search.

| Operation, one trial per executable | Previous | Refactor |
| --- | ---: | ---: |
| First visible application frame | 3.289 s | 3.313 s |
| Cold public search | 1.183 s | 1.119 s |
| Warm, different public search | 358 ms | 359 ms |
| Cached public search | 47 ms | 62 ms |
| Settled idle working set | 109.7 MiB | 48.2 MiB |
| Cached different search after idle | 107 ms | 154 ms |

The idle working-set reduction was **56%** after the metadata helper was released;
the cached request did not restart it. Active helper working set remained about
109 MiB with Glass. Final private memory after that cached request was 30.1 MiB.
CPU averaged 0.36% of one core over the 35-second idle phase, including helper
shutdown. Short initial idle samples registered 0%. These are snapshots, not
sustained playback or leak-soak measurements. A reliable speed improvement is
not established from the small sample or the earlier three-trial search runs.

Earlier plain-theme measurements using an intermediate candidate showed 85.1
to 24.0 MiB settled idle working set, about 72% lower. They do not measure the
final release. Live connected-library diagnostics returned playlists in 279 ms
and Liked Songs in 296 ms, with cached repeats below 1 ms. Public search worked,
but actual YouTube audio was blocked by HTTP 429/sign-in/connection errors and
the saved Spotify login was revoked/expired. Playback and next-track latencies
and steady playback memory remain unverified.

[Sanitized process measurements](docs/performance/v0.5.0-conpty.json) retain the
phase summaries without local paths or credentials. The harness is available
in the source repository at
[scripts/benchmark.py](https://github.com/braces157/tutify/blob/v0.5.0/scripts/benchmark.py).
It requires Python 3.10+, `pywinpty`, `psutil`, `pyte` and `wcwidth`. Use an isolated
dependency directory and a separate app-data fixture. Populate the fixture's
`Programs/Tuitify/tools` with installed tools before running:

```powershell
python scripts/benchmark.py --exe target/release/tuitify.exe --root work/benchmark-fixture --out work/benchmark.json --trials 3 --idle-seconds 35
```

The fixture must never be the real LOCALAPPDATA directory. `--appearance` copies
only theme/background settings from a config file. `--audio` is a muted optional
probe; it may fail on upstream restrictions. The harness validates exact search
queries and visible rows; it controls and closes only its own player session.

## Offline rendering

Run on an otherwise idle machine with the locked dependencies and optimized build:

```powershell
cargo test --release --locked benchmark_render_scaling -- --ignored --nocapture --test-threads=1
```

This ignored test uses a 120×35 Ratatui TestBackend, synthetic track metadata,
a paused player, and 50, 500, or 5,000 queue/catalog entries. Visible queue
metadata is cached. The filtered catalog matches all rows. One warm-up draw
builds the filter index; each result averages 100 subsequent draws. This measures
steady-state row construction and buffer rendering, not first-filter latency,
actual terminal output, Spotify, audio, persistence, or whole-process CPU/RAM.
The production paused event loop does not continuously perform these draws.

Local Windows x86_64 results on 2026-09-06, Rust 1.95.0:

| Entries | Queue frame | Cached filtered-catalog frame |
| --- | ---: | ---: |
| 50 | 0.185 ms | 0.211 ms |
| 500 | 0.164 ms | 0.198 ms |
| 5,000 | 0.161 ms | 0.204 ms |

The review's earlier queue-only probe measured 0.181 / 0.919 / 8.563 ms for the
same entry counts and terminal size, using 30 measured draws with all metadata
cached. The updated probe uses 100 draws and caches the visible queue window
to match the bounded cache design. These local samples demonstrate the removal
of full-list work from each frame; they are not a statistical performance SLA.

For a published comparison, repeat the command at least five times and report
median and range, CPU model, logical-core count, power mode, OS, Rust version,
commit, terminal dimensions, and whether the machine was under other load.
Keep raw output with that report. CI runs correctness tests, not timing gates
whose results depend on shared runner load.

## Buffered persistence and statistics refresh — 2026-09-30

The v0.3.1 review used the same synthetic data, repetitions, optimized build,
and Windows machine for before/after microbenchmarks (Rust 1.95.0). These are
local samples, not whole-process latency, account playback, or a hardware-independent SLA.

| Operation | Rows | Before | After |
| --- | ---: | ---: | ---: |
| Queue snapshot save | 5,000 | 96.311 ms | 3.437 ms |
| Queue snapshot save | 50,000 | 941.550 ms | 7.443 ms |
| Statistics refresh, Plays | 5,000 | 11.028 ms | 1.508 ms |
| Statistics refresh, Plays | 50,000 | 137.099 ms | 24.270 ms |
| Statistics refresh, Time | 50,000 | 141.321 ms | 21.089 ms |
| Statistics refresh, Title | 50,000 | 143.608 ms | 21.103 ms |

Queue saves now buffer serialization before flushing, syncing, and atomic
replacement. Statistics refresh normalizes titles once, filters before cloning,
and sorts once. Playback and queue semantics are covered by correctness tests.

```powershell
cargo test --release --locked benchmark_ -- --ignored --nocapture --test-threads=1
```

This also runs the existing render and Mix Builder probes. Compare repeated runs
under equivalent load; timings are not CI pass/fail thresholds.

## Actual TUI process measurements

Use a release build and a real Windows Terminal session. Prepare queues of
50, 500, and 5,000 entries. Record each workload separately:

1. Paused, names hydrated, unchanged screen, for 60 seconds.
2. Normal playback with the decorative mini-visualizer, for 60 seconds.
3. Expanded visualizer and lyrics, separately, for 60 seconds each.
4. Cold metadata restore versus warm restore; note API requests and failures.
5. Filtering while typing and navigation across a large queue. Measure event-to-
   visible-frame latency as well as filter computation, especially p95/p99.

Windows Performance Recorder/Analyzer can attribute CPU, allocations, disk I/O,
and input stalls. Process Explorer can sample working set and private bytes.
Report both memory metrics explicitly, and distinguish a percentage of one
logical core from a percentage of total machine CPU. Exclude auth/browser setup
from warm-start timing, and define startup as process creation to the first
usable frame. Report cold-start/auth timings separately. Never infer memory
usage from the EXE or ZIP size.

The offline tests do not establish current audible playback, network-outage
recovery, output-device reconnection, or real-terminal drawing latency. The
opt-in acceptance commands in README remain available for those environments.

## Website

Install locked Node dependencies, build the static CSS, and run the browser tests
using the commands in README. Use the local site URL printed by its server for
Lighthouse or browser Performance tools. Record browser/version, viewport,
device/network throttling, cache state, and at least five cold navigations.

Track LCP, CLS, long tasks, transferred JS/CSS, and main-thread work. Profile the
demo while onscreen, scrolled offscreen, paused, and with reduced motion enabled.
The site should not request runtime Tailwind or remote font services. Interaction
tests verify behavior; they are not a substitute for Core Web Vitals measurements.

Only publish competitor numbers when the same hardware, workload, duration,
memory definition, and software versions were measured under equivalent conditions.
