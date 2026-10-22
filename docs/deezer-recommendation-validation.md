# Deezer recommendation validation — 2026-10-21

The native Deezer catalog path used by Radio, Smart Shuffle, Mix Builder and
Autoplay now shares a bounded, balanced recommendation selector. The Spotify
similarity path already had artist caps; the newer native path did not.

## Reproduced FIESTA failure

For FIESTA by IZ*ONE (Deezer track 1756558947, artist 53330262), live artist-top
responses returned 20 Red Velvet tracks, one each for OH MY GIRL, Weeekly and
fromis_9, and zero for Kep1er. Interleaving those five lists exhausted the small
lists and filled the rest with Red Velvet. Normal name searches returned much
fuller catalogs for the sparse artists.

## Changes

- Fill sparse or excluded artist-top pools with public name-search results.
  Require the requested numeric artist ID in the primary credit; reject covers,
  same-name artists and guest-credit detours.
- Keep at most three songs per credited artist per 15-song batch, and at most
  two for the seed artist. Shared collaborator credits count toward the caps.
  Sparse coverage produces a shorter batch instead of relaxing these limits.
- Obtain enough usable pools, inspecting up to twelve directly related artists
  when earlier pools are empty or exhausted. Refill from the original seed,
  rotate related artists, and use overlapping pages to retain unselected songs.
- Balance remaining pool sizes to avoid an unnecessary repeated-artist tail.
  Consider the previous queue tail and existing artist counts when ordering.
- Reject already queued IDs and alternate editions of the same recording,
  unavailable/empty metadata, and existing special-version exclusions.
- Reuse the public response cache and service cooldowns. Exclusion checks use
  sets so long queues do not require repeatedly scanning every queued recording.

These are artist-based recommendations. The catalog does not provide reliable
song-level mood/energy similarity here. Variety and identity checks do not prove
that every suggestion suits every listener, and finite tests cannot cover every
song or future provider response. Live catalog checks do not stream audio.

## Repeatable checks

```powershell
cargo test --locked
cargo test --locked live_deezer_recommendation_quality_matrix -- --ignored --nocapture
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

The seeded synthetic check exercises 2,000 dense, sparse and exhausted pools,
checking capacity, recording uniqueness, caps and the mathematically unavoidable
minimum of adjacent artist repeats, including the previous queue tail. Mocked
provider checks cover the FIESTA distribution, verified search, cache reuse,
refills, duplicate related IDs, guest-credit noise, finite empty-catalog request
budgets and no-related-artist coverage. The app test exercises search, Radio,
completion, refill and Smart Shuffle through background handlers.

The live matrix uses twelve seeds across K-pop, Japanese and Vietnamese music,
Western pop, rock, rap, metal, electronic and jazz. Each seed gets an initial
batch, two refills and a fresh Smart-style request after excluding the Radio
results. Assertions cover identity/recording exclusions, artist caps, minimum
initial variety and nonempty, diverse refills.

On 2026-10-21, all twelve live seeds passed: FIESTA, Psycho, Creep, Castle on the
Hill, Mưa Tháng Sáu, だから僕は音楽を辞めた, 晩餐歌, good 4 u, HUMBLE., Enter Sandman,
One More Time and So What. Every initial batch had 15 songs across six primary
artist IDs. Both refills and the Smart-style request returned 15 fresh songs for
each seed: 48 batches and 720 suggestions checked in total. The artist cap held
throughout; refills had five or six distinct primary artist IDs. These counts
describe Deezer identities, which can include separate catalog identities for
the same musician, rather than a manually resolved universal artist database.

After installation, restart Tuitify and start a fresh search/Radio session.
Restoring an old queue preserves the old recommendations.

## Delivery

The full locked suite passed: 641 unit tests plus seven build-identity tests;
29 opt-in tests were skipped. The live matrix above was run separately and
passed. Formatting, Clippy with warnings denied, diff whitespace checks and
`cargo build --release --locked` passed.

The release was installed through `scripts/install.ps1` to the canonical
`%LOCALAPPDATA%\Programs\Tuitify` directory and the four existing alternate launch
locations (project root, its previous `bin` installation, `.local\bin` and
`.cargo\bin`). All five executable SHA-256 hashes matched the release:

`2EF249D1C277E29A517A8757FB90AD00914B5E1FBA1D11E688A96D668D111535`

Both `Get-Command tuitify -All` and `where.exe tuitify` were inspected. The saved
user PATH puts the canonical directory first; resolving with fresh machine and
user PATH values selects the canonical executable and matches that hash. The
running Tuitify process was left playing and still requires a restart.
