# Reproducing performance measurements

Tuitify does not promise a fixed RAM footprint, startup time, or CPU percentage.
Those depend on the terminal, hardware, audio device, network, cache, and library.
Executable size is not process memory usage.

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
