# App Store screenshots

Drop device screenshots in and run:

    ./make-screenshots.sh ~/Downloads/IMG_*.png

Argument order becomes store order (`01.png` first), so pass them in the
order you want them to appear on the listing rather than whatever order the
shell globbed.

That imports them to `originals/` (renumbered 01..NN in argument order) and
generates every size. Re-run with no arguments to regenerate from `originals/`.

| Folder | Size | Needed? |
|---|---|---|
| `iphone-6.3/` | 1206×2622 | **yes**: "iPhone with Dynamic Island (medium display)", the iPhone 17/16/15 Pro size, required since October 2026. Our captures are this size natively |
| `iphone-6.9/` | 1320×2868 | optional: "Dynamic Island (large display)". Was the required size until October 2026 |
| `iphone-6.5/` | 1284×2778 | optional: Connect scales it from 6.9 when missing |
| `ipad-13/` | 2064×2752 | only if the app ships for iPad — `build-ios.cjs` sets `TARGETED_DEVICE_FAMILY "1"` (iPhone only), so Connect won't offer an iPad slot |

## Captions

`captions.txt` gives each original a headline and a subhead, drawn above a
scaled-down capture rather than shipping the bare screen:

    01|Up to four cards|Work, personal, side hustle. Kept separate.

A screenshot with no line there is rendered full-bleed and uncaptioned.
`SKIP_CAPTIONS=1` does that for the whole set.

**The text is rendered by `caption-render.swift`, not by ImageMagick.** The
ImageMagick installed here is built *without freetype* --

    magick -version | grep Delegates    # no "freetype" in the list

-- so `-annotate` and `-draw text` don't render anything. They fall through to
Ghostscript and the run dies with `sh: gs: command not found`, which looks
like a missing dependency and isn't one worth chasing: installing ghostscript
only buys the PostScript base-35 fonts, not SF Pro. CoreText draws the same
system font the app itself draws, so the caption and the screen below it are
set in the same typeface. It needs `swiftc` (Xcode), which this machine
already needs for `build:ios` anyway.

Headlines over about 34 characters wrap to two lines. That lays out correctly,
it just pushes the capture down and shrinks it.

## Things that have bitten us

**TestFlight back-link, and when NOT to mask it.** Screenshots taken straight
after opening the app from TestFlight carry a "◀ TestFlight" return link in
the status bar. It's iOS chrome, not the app, and it tells every viewer the
listing art came from a beta. The script paints it out; `SKIP_TF_MASK=1`
disables that.

The rectangle is the *measured* extent of that text on a 1206×2622 capture
(iPhone 16 Pro) -- glyphs at x 32..232, y 102..140, masked as 24,98 + 216×45.
It was 0,96..268,146 until September 2026, which was guesswork and overshot
the text by roughly 30px per side.

Two things used to need checking by hand before every run. The script now
checks both:

- **A capture with no back-link.** With the link, iOS lays the status bar out
  on *two* lines and the clock sits high. Without it the clock drops back to
  one line, landing squarely inside the mask rect, so masking slices it in
  half. The script measures the height of the bright band in the status bar
  (79px with the link, 36px without) and skips the mask when it's short.
  Counting bright *pixels* does not work here and was the first thing tried:
  the clock alone has plenty, so a no-back-link capture reads as a back-link.
  This app's whole set is that case; see below.
- **App content scrolled under the status bar.** Then the link and the app's
  own pixels *overlap*, and no rectangle removes one without damaging the
  other. Detected by sampling the band just below the zone. The script warns
  and names the file; the fix is a retake, not a coordinate tweak.

A capture that isn't 1206×2622 is left unmasked with a warning, since the
coordinates are per-device.

**This app's whole set is the no-back-link case.** All four originals were
captured outside TestFlight, so the script reports "no TestFlight back-link,
leaving the status bar alone" for each one and never touches the status bar.
`SKIP_TF_MASK=1` is no longer needed here; it used to be, and setting it does
no harm.

**Current set: 0.2.3 build 5, October 2026.** Recaptured from IMG_0407,
0403, 0404 and 0406 in that order (card, My Link Cards, New Link Card
chooser, edit view). The card shows the `linkityl.ink` short link. The
first tries at the card and edit view were captured scrolled under the
status bar and had to be retaken scrolled to the top.

**Other iOS overlays.** Autofill suggestion pills and notification banners sit
*over* app content, so they can't be masked cleanly. Retake the screen.

**Test data.** Placeholder content like `bar@foo.com` or a bio of "asdf" reads
as a debug build in a store listing. Worth populating a card properly before
capturing.

**No alpha.** App Store Connect rejects screenshots with transparency. The
script flattens on the way out.
