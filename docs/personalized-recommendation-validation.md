# Local recommendation personalization — 2026-10-22

> **Dated record.** This describes the taste profile as of the date in the title.
> For current user-facing behavior see
> [How playback and recommendations work](../README.md#how-playback-and-recommendations-work);
> for current verification see [VALIDATION.md](../VALIDATION.md).

Related-artist discovery previously balanced artists and excluded queued
recordings, but did not rank candidates using the listener's preferences.
Radio, Autoplay, Smart Shuffle and Mix Builder now apply a local taste profile.

## Signals and ranking

- Existing aggregate play counts supply a bounded, logarithmic artist preference.
  Alternate editions of a recording do not multiply this evidence.
- New positive feedback requires actual accounted listening: 60% of the duration,
  capped at 90 seconds, with a three-second minimum. Seeking does not manufacture
  listening time. Each playback generation contributes at most one event.
- Only an intentional Next while playing supplies negative feedback, after at
  least three seconds but before both 30 seconds and one third of the duration.
  Pauses, seek actions, failures, loading skips and very brief skips are neutral.
- Recent feedback decays with a fourteen-day half-life and is ignored after
  ninety days. One skip has limited artist influence. Two early skips since the
  latest positive listen, within thirty days, suppress that recording for seven
  days after the latest skip. A subsequent sustained listen clears suppression.
- A recording heard within the last day receives a fading ranking penalty;
  it remains available when catalog coverage is sparse. Familiar artists can
  supply fresh recordings instead of repeatedly supplying their top songs.
- Artist spacing takes precedence over taste score. Remaining artist capacity
  prevents postponing a large pool into an unnecessary repeated-artist tail.
  Album repetition is a small tie-breaking penalty. Stable ties preserve the
  provider's candidate ordering, including an unchanged cold start.

Native Deezer candidate pools apply eligibility and taste ranking before the
existing two-seed/three-related-artist caps. This lets other recordings replace
repeatedly skipped or recently heard top tracks. Other catalog routes retain
their existing retrieval methods and personalize the returned candidates locally.
All four app modes recheck current feedback when a response arrives, so a skip
made during a pending request still applies. User-selected queue songs remain.

Discovery remains anchored to the original seed and directly related artists.
There is no broad genre search, recursive walk through generated suggestions,
new external service, audio-feature model, or listener-scale collaborative model.
These are heuristic improvements; automated invariants do not establish Spotify
parity or prove a subjective improvement for every song and listener.

## Saved data

`stats.json` keeps its version-1 aggregate counts and adds an optional `feedback`
array. Old files load with an empty array. At most 256 events retain song/artist
metadata, a timestamp and the outcome. They share the existing atomic statistics
writer, backup/restore and statistics reset. No feedback is sent to providers or
diagnostic reports. Invalid feedback is rejected without exposing raw IDs in
state inspection. Preference snapshots share public metadata caches without
putting preference data into requests or shared cached responses.

## Verification

```powershell
cargo test --locked --quiet
cargo test --locked live_deezer_recommendation_quality_matrix -- --ignored --nocapture
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
cargo build --release --locked
```

Focused regressions cover cold starts, positive history, fatigue, repeated skips,
cooldown recovery, separate same-name artist identities, feedback decay, queue
boundary spacing, bounded/legacy serialization, real statistics save/load/reset,
generation-safe transport accounting, neutral pause/seek/failure actions, current
feedback in all four modes, and personalized pre-cap Deezer selection with cache
reuse. The live matrix adds personalized batches to the twelve existing seeds,
using synthetic local feedback without streaming audio or writing saved state.

## Results and delivery

On 2026-10-22, all 653 regular unit tests and seven build-identity integration
tests passed; 29 opt-in checks were skipped. The live matrix was run separately
and passed all twelve seeds, covering initial, personalized, two refill and
Smart-style batches. All sixty batches returned fifteen songs: 900 suggestions
checked in total. Every personalized batch excluded both repeatedly skipped
recordings. These are metadata/algorithm checks, not a listening preference study.

Clippy with warnings denied, formatting, diff whitespace validation and the
locked release build passed. The existing installer updated the canonical
installation plus the project-root, previous `Programs/Tuitify/bin`, `.local/bin`
and `.cargo/bin` copies. All five SHA-256 hashes match the release:

`B55291C80AD0373E12B8CD157ACCAA43FE4FBEAAB66DAADF433AEE50EC9B327F`

`Get-Command tuitify -All`, `where.exe tuitify`, the saved user PATH and running
process paths were inspected. No player process was running during installation.
The canonical directory is first in the saved user PATH; fresh machine/user PATH
resolution selects its verified executable. Unrelated PATH entries were preserved.
Start a fresh Radio or Smart Shuffle session to use newly ranked candidates;
restoring a queue preserves its existing recommendations.
