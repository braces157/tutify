# Website design and maintenance

The static website in `docs/` introduces Tuitify and links to the published
Windows build. It is separate from the native Rust application. The standalone
browser player uses sample data and simulated playback; it does not stream audio,
connect accounts, or write the native application's saved state.

## Pages and assets

| File | Responsibility |
| --- | --- |
| `index.html` | Download page, native captures, feature descriptions, installation, release boundary, FAQ |
| `player.html` | Interactive, silent browser player with queue, search, lyrics, spectrum, themes, and Help |
| `assets/home.css` | Shared font, palette, navigation, homepage layout, responsive rules, motion defaults |
| `assets/home.js` | Homepage motion preference, reveal state, launch-command copy and clipboard fallback |
| `assets/listening-scene.css`, `assets/listening-scene.js` | Record-sleeve hero and motion pause, reduced-motion and visibility handling |
| `assets/player-page.css` | Browser player layout and controls from 320px through desktop widths |
| `assets/demo.js` | Sample catalog, player state, scoped shortcuts, keyboard tabs, radio and lyrics |
| `assets/tailwind.css`, `assets/tailwind.compiled.css` | Retained utility stylesheet, generated with the pinned Tailwind build |
| `assets/site.js`, `assets/motion.js` | Retained helpers for the earlier combined website/demo layout |

Both pages load their fonts, styles, scripts and images locally. A fresh clone
must include every referenced asset. Assets from unused design experiments are
not required by the current pages.

## Visual system

Space Grotesk is served from `assets/fonts/SpaceGrotesk.ttf`. The homepage uses
Space for headings and body copy. The terminal simulation may use monospace for
its interface. The core palette is:

| Role | Value |
| --- | --- |
| Background | `#090d0c` |
| Panel | `#111715` |
| Raised panel | `#171e1b` |
| Primary text | `#f2f4ec` |
| Secondary text | `#a5ada4` |
| Accent and focus | `#d6ff5e` |
| Divider | `#2a342f` |

Keep the record sleeve and terminal capture as the main homepage image. Use the
same navigation, typography and palette on the player page. On phones, stack the
content and keep transport, search, volume and tabs inside the player container.
The browser demo cycles through six accents independently of the website palette.

## Content and release accuracy

Download URLs and version labels refer to the published v0.7.0 release. The
homepage's source-build note explicitly distinguishes unreleased Monochrome,
lyrics and audio improvements from that download. Do not bump download links
until matching release assets exist.

Native demo captures contain fictional tracks and simulated playback. The Glass
lyrics image is a v0.6.0 live capture. Captions retain those versions and limits.
The record artwork is a code-drawn illustration. The browser player carries an
explicit sample-data and no-audio notice; it is not an authenticated web client.
No stock photograph is used by the current pages.

The README owns the complete command reference and provider requirements. Link
to it instead of claiming that the executable needs no provider tools. Explain
that the launch command requires installation on PATH; standalone downloads can
be opened directly. Spotify Premium is needed only for Spotify audio.

## Accessibility and motion

Use native links, buttons, details and range inputs. Keep visible keyboard focus,
meaningful image alternatives, skip links, scoped player shortcuts, selected tab
states and live announcements. Clipboard failures leave the command readable.

The homepage motion preference follows reduced motion until the visitor chooses
an override, and persists that choice locally. Record motion pauses offscreen,
on request and when the document is hidden. The player stops decorative motion
when playback is paused and respects reduced motion. Essential content and native
download links remain usable without JavaScript.

## Build and verification

```powershell
npm ci
npm run build
npm test
npm run serve
```

The preview server serves both `/` and `/docs/index.html`; the player is reachable
at `/player.html` and `/docs/player.html`. Open the actual pages in Edge when
checking a layout change. Review phone, tablet and desktop widths. Browser tests
cover download versions, local asset loads, installation copy, motion preferences,
clipboard fallback, player interaction and responsive controls. CI also requires
the generated Tailwind CSS to match the committed file.

See [the documentation map](README.md) for current engineering and validation
records. Older design review notes describe earlier iterations.
