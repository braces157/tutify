# Smart Shuffle and Track Radio validation

Updated 2026-09-15 on Windows.

## What was wrong

Genre/era searches produced unrelated chart pop in Japanese queues. Replacing
those searches with same-artist catalogs removed the outliers but returned
mostly one artist. The old acceptance test explicitly allowed that result.
Neither queue size nor an artist credit alone proved useful discovery.

## Current implementation

Radio and Smart Shuffle use Deezer's public related-artist feed, with Spotify
supplying the actual tracks and audio. Artist connections inferred from guest
credits are no longer the production discovery source.

1. Resolve the seed artist in Deezer. Prefer an exact name. For multiple exact
   names, require a matching seed recording rather than choosing the most popular
   result. Cache identities by Spotify artist ID so same-name artists stay separate.
2. Read related artists from that artist's feed. No broad genre query and no
   recursive recommendation-to-recommendation walk is used.
3. Query at most six neighbouring artists in Spotify to obtain five usable
   artist pools, plus one page for the root. Verify matching artist credits and
   stable Spotify artist IDs; reject ambiguous or unrelated search results.
4. Exclude unavailable songs, alternate recordings already queued, remixes and
   special versions. Rank with artist spacing and cap Radio batches at three
   songs per primary artist, up to fifteen suggestions.
5. Preserve the original seed for refills. Smart Shuffle inserts suggestions
   after three original tracks and prefers different artists around each slot.
   Radio and Smart suggestions do not become automatic seeds.
6. Cache related artists for thirty minutes and Spotify candidate pages across
   mode changes. Limit external metadata calls to one per second. Honor 429
   cooldowns and cache failures briefly instead of retrying in a loop.
7. When similarity fails, preserve the queue and display an error. Do not pad a
   failed discovery run with a full single-artist catalog. Radio failures remain
   visible in the queue header after playback status updates.

The UI labels this source **Similar artists • Deezer**. This provides variety
based on artist similarity; it does not reproduce Spotify's personalized ranking
or guarantee track-level mood/energy similarity.

Deezer receives artist names and, only when needed for disambiguation, a seed
song title. It does not receive Spotify bearer tokens or user listening history.
The existing Spotify recommendations path for supported Mix Builder accounts
remains available; its fallback uses the same new discovery engine.

## Real audio acceptance

Command:

```powershell
cargo test live_tuitify_multiple_song_acceptance -- --ignored --nocapture
```

The test uses production input routing, live Deezer and Spotify responses,
background jobs, queues, native TUI rendering and real Windows/librespot audio
at 10% volume. It asserts at least four distinct primary artists in each initial
recommendation batch, at most three songs per primary artist, no duplicate
recordings, and Smart suggestions from at least two artists other than the seed.

| Seed | Initial suggestions | Suggested songs actually streamed | Refill | Smart examples |
| --- | ---: | --- | ---: | --- |
| good 4 u — Olivia Rodrigo | 15 | Heather — Conan Gray; Espresso — Sabrina Carpenter | +15 | Too Well — Reneé Rapp; Maniac — Conan Gray |
| 月面着陸計画 - Live — tuki. | 15 | ヨワネハキ — MAISONdes; Bunny Girl — AKASAKI | +15 | トウキョウ・シャンディ・ランデヴ — MAISONdes; Spica — Rokudenashi |
| 猫日 — suis from Yorushika | 15 | Shout Baby — Ryokuoushoku Shakai; Marigold — Aimyon | +15 | 君はロックを聴かない — Aimyon; 愛唄 — GReeeeN |

The Japanese results did not contain the unrelated Taylor Swift / Olivia songs
from the screenshots. Each case also passed completion-driven track advance,
original-seed retention, queue validation and the Smart indicator/render check.

The first run passed Olivia and tuki., then failed on suis: punctuation
normalization conflated two Deezer catalog entries and the error was hidden by
a later playback status message. Exact-name preference and persistent Radio
errors corrected those defects. A focused full-audio rerun for suis passed:

```powershell
$env:TUITIFY_LIVE_ARTIST = 'suis from Yorushika'
cargo test live_tuitify_multiple_song_acceptance -- --ignored --nocapture
```

The three cases passed across these runs; the complete unfiltered invocation
was not repeated after the focused fix. These observations demonstrate working
variety and playback, not a subjective listening-quality score.

The saved Spotify login was initially expired/revoked. Both login steps completed
before the real audio checks. No live rate limit occurred during these checks.

## Automated coverage

Final checks: 301 regular tests passed, 11 opt-in tests ignored; Clippy with
warnings denied, formatting and diff whitespace checks passed.

Regression checks cover:
- Multiple-artist Radio and Smart flows through application input/background handlers.
- Per-artist caps, recording deduplication and cached refills.
- Unrelated Spotify search hits failing identity checks.
- Similarity failures preserving the seed queue rather than returning same-artist padding.
- One-off name variants, ambiguous artist names, and identity cache isolation.
- Absence of Spotify authorization headers on external metadata requests.
- External 429 cooldown and failure caching.
- Radio errors surviving playback updates and clearing on a new Radio session.

The retained catalog-only acceptance test is
`live_japanese_and_olivia_app_catalog_acceptance`; it uses synthetic playback
acknowledgements and is not evidence of audible playback.

## Delivery

Release build installed and SHA-256 verified on the canonical PATH location,
the previous bin location, and the existing project-local launch copy:

`F63B3C65D9B1541ED76ECC1521A27AC31851398B3589FFF4CD01C47040B127AF`

Follow root AGENTS.md: build the release, install both the canonical executable
and the previously used `Programs/Tuitify/bin/tuitify.exe`, then compare SHA-256.
Restart Tuitify and start a fresh search/radio queue. Restoring an old queue does
not replace suggestions generated by an earlier algorithm.

## Public provider references

- [Deezer related artists for tuki.](https://api.deezer.com/artist/229176215/related?limit=10)
- [Deezer artist search](https://api.deezer.com/search/artist?q=Olivia%20Rodrigo&limit=10)

ListenBrainz was evaluated during investigation but lacked similar-artist
coverage for tuki.; it is not a runtime dependency.
