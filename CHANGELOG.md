# Changelog

## 0.3.1 — 2026-09-30

- Fix Vietnamese Radio lookup when catalogs differ in Latin accents. Confirm
  aliases with the seed recording and retain exact-name/artist-ID safeguards.
- Fix duet lyrics by retrying the primary artist after a missing full-credit
  result, validating the returned title, artist, and duration. Preserve Unicode
  accents and normalize canonically equivalent character sequences.
- Retry temporary lyrics HTTP 502–504 failures once with a short delay, honor
  longer cooldowns, and show clear server/rate-limit errors with F5 retry.
- Complete Statistics and Mix Builder mouse controls, including wrapped controls,
  long searches, long Unicode recipe names, and scrollable details.
- Preserve invalid statistics/recipe files by stopping startup before workers run.
- Buffer atomic JSON saves and share immutable snapshots instead of cloning
  collections again in writer tasks. Refresh statistics with one normalization
  and sort pass and update hydrated metadata per track.
- Correct Glass refresh deadlines and use the actual output-device sample rate
  for FFT frequency bands.
- Restrict artist-catalog fallback to HTTP 403 and validate Spotify artist IDs.
- Refresh the website, guides, validation record, benchmark procedures, and
  release downloads. Include linked documentation and the installer in the ZIP.

## 0.3.0 — 2026-09-15

- Introduce the Glass theme, custom wallpaper backgrounds, portable Unicode
  rendering, and a dedicated native-resolution Windows Terminal profile.
- Add `background`, `--glass`, and `--glass-window` controls and readable queue,
  lyrics, and visualizer surfaces over detailed artwork.

Earlier release history is available in [GitHub Releases](https://github.com/braces157/tutify/releases).
