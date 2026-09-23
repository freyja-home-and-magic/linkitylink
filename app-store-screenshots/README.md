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
| `iphone-6.9/` | 1320×2868 | **yes** — the only size an iPhone app must supply; App Store Connect scales it down for every smaller iPhone |
| `iphone-6.5/` | 1284×2778 | optional |
| `ipad-13/` | 2064×2752 | only if the app ships for iPad — `build-ios.cjs` sets `TARGETED_DEVICE_FAMILY "1"` (iPhone only), so Connect won't offer an iPad slot |

## Things that have bitten us

**TestFlight back-link, and when NOT to mask it.** Screenshots taken straight
after opening the app from TestFlight carry a "◀ TestFlight" return link in
the status bar. It's iOS chrome, not the app, and it tells every viewer the
listing art came from a beta. The script paints it out; `SKIP_TF_MASK=1`
disables that. Only OS chrome is touched, never anything the app drew.

The catch: that back-link sits where the **clock** normally is, so the mask
rectangle covers the clock on any capture that has no back-link. Masking
those leaves a black box where the time should be. Check the top-left corner
before running:

    magick shot.png -crop 700x200+0+40 +repage -resize 200% /tmp/check.png

If you see a time rather than "◀ TestFlight", run with `SKIP_TF_MASK=1`. The
September 2026 Linkitylink set was captured outside TestFlight and needed it.
The mask coordinates assume a **1206×2622** capture (iPhone 16 Pro), which is
what both app's sets have been so far; a different device size needs them
re-measured.

**Other iOS overlays.** Autofill suggestion pills and notification banners sit
*over* app content, so they can't be masked cleanly. Retake the screen.

**Test data.** Placeholder content like `bar@foo.com` or a bio of "asdf" reads
as a debug build in a store listing. Worth populating a card properly before
capturing.

**No alpha.** App Store Connect rejects screenshots with transparency. The
script flattens on the way out.
