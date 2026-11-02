#!/bin/bash
#
# Produces every App Store screenshot size from the files in originals/.
#
#   ./make-screenshots.sh                 # regenerate from originals/
#   ./make-screenshots.sh ~/Downloads/IMG_*.png   # import new ones first
#
# Sizes, per App Store Connect's spec as of October 2026
# (developer.apple.com/help/app-store-connect/reference/app-information/
# screenshot-specifications):
#
#   iphone-6.3   1206x2622   REQUIRED: "iPhone with Dynamic Island (medium
#                            display)", the iPhone 17/16/15 Pro size. Also
#                            the native size of our captures, so no scaling.
#                            Until October 2026 the required size was 6.9.
#   iphone-6.9   1320x2868   "Dynamic Island (large display)", optional.
#                            Connect scales smaller iPhones from these two.
#   iphone-6.5   1284x2778   "Face ID (large display)", optional; Connect
#                            scales it from 6.9 when it's missing.
#   ipad-13      2064x2752   only relevant if the app ships for iPad.
#                            Linkitylink sets TARGETED_DEVICE_FAMILY "1"
#                            (iPhone only) in build-ios.cjs, so App Store
#                            Connect won't even offer an iPad slot. Generated
#                            anyway so the set is ready if that changes.
#
# No alpha channel: App Store Connect rejects screenshots with transparency,
# the same rule that applies to the 1024 marketing icon.

set -euo pipefail
cd "$(dirname "$0")"

# Sampled from the screenshots themselves — the app's dark-mode ground. Used
# to pad the iPad canvas, where a portrait phone shot (aspect 0.46) can't fill
# an iPad (0.75) without either huge letterboxing or cropping content away.
PAD="#1a1a1a"

command -v magick >/dev/null || { echo "Need magick (brew install imagemagick)" >&2; exit 1; }

# Importing: copy any files passed in, renumbered 01..NN in argument order.
if [ "$#" -gt 0 ]; then
    echo "Importing $# screenshot(s) into originals/..."
    rm -f originals/*.png
    i=1
    for src in "$@"; do
        printf -v n "%02d" "$i"
        cp "$src" "originals/$n.png"
        echo "  $(basename "$src") -> originals/$n.png"
        i=$((i + 1))
    done
fi

shopt -s nullglob
originals=(originals/*.png)
[ "${#originals[@]}" -gt 0 ] || { echo "No files in originals/" >&2; exit 1; }

# ── TestFlight back-link ─────────────────────────────────────────────────────
#
# Screenshots taken from a TestFlight build carry a "◀ TestFlight" return link
# in the status bar. It's iOS chrome, not part of Linkitylink, and it advertises
# that the listing's screenshots came from a beta.
#
# The rectangle is the MEASURED extent of that text on a 1206x2622 capture
# (iPhone 16 Pro): the glyphs run x 32..232, y 102..140, and TF_* below adds a
# few pixels of margin. It used to be 0,96..268,146, which was guesswork and
# overshot the text by about 30px on each side.
#
# It sits on flat #1a1a1a with nothing behind it, so painting over it is
# pixel-exact rather than a visible patch. The originals are untouched, so
# rerunning without SKIP_TF_MASK=1 restores whatever you prefer.
#
# This only removes OS chrome. It doesn't alter anything the app drew, which
# would misrepresent the product -- and the two guards below refuse to run
# when that promise can't be kept, rather than leaving you to eyeball it.
TF_X=24; TF_Y=98; TF_W=216; TF_H=45
CAPTURE_W=1206; CAPTURE_H=2622

mask_testflight() { # $1 = file, edited in place; $2 = name for messages
    [ "${SKIP_TF_MASK:-}" = "1" ] && return 0

    # The coordinates are per-device. On any other capture size they point at
    # the wrong pixels, so don't guess -- re-measure.
    local dims
    dims=$(magick identify -format '%wx%h' "$1")
    if [ "$dims" != "${CAPTURE_W}x${CAPTURE_H}" ]; then
        echo "  ! $2 is $dims, not ${CAPTURE_W}x${CAPTURE_H} -- mask coords don't apply, skipping" >&2
        return 0
    fi

    # Is the back-link actually there? With it, iOS lays the status bar out on
    # TWO lines -- clock above, "◀ TestFlight" below -- and the bright band in
    # the status bar runs ~79px tall. Without it the clock goes back to ONE
    # line and sits LOWER, landing inside the mask rect, with a band of ~36px.
    #
    # Counting bright pixels does not tell these apart: the clock on its own
    # has plenty, so a no-back-link capture reads as a back-link and the mask
    # slices the clock in half. Measuring the band's HEIGHT does tell them
    # apart, and by a wide margin. (Measured across both apps' sets: 79 with
    # the link, 36 without.)
    local band_h
    band_h=$(magick "$1" -crop 216x120+24+50 +repage -threshold 90% \
        -format '%@' info: 2>/dev/null | sed -nE 's/^[0-9]+x([0-9]+).*/\1/p')
    if [ "${band_h:-0}" -lt 55 ]; then
        echo "  · $2 has no TestFlight back-link, leaving the status bar alone"
        return 0
    fi

    # App content scrolled up under the status bar overlaps the link itself, so
    # no rectangle can remove one without damaging the other. Detected by
    # looking at the band just BELOW the zone: background on a clean capture,
    # app pixels when something has scrolled up into it.
    local below
    below=$(magick "$1" -crop "${TF_W}x18+${TF_X}+$((TF_Y + TF_H))" +repage \
        -format '%[fx:maxima*255]' info:)
    if [ "${below%%.*}" -gt 40 ]; then
        echo "  ! $2 has app content under the status bar; the mask will clip it." >&2
        echo "    Retake this screen scrolled so nothing sits beneath the clock." >&2
    fi

    magick "$1" -fill "$PAD" -draw \
        "rectangle $TF_X,$TF_Y $((TF_X + TF_W)),$((TF_Y + TF_H))" "$1"
}

# ── Captions ─────────────────────────────────────────────────────────────────
#
# captions.txt gives each original a headline and subhead, drawn above a
# scaled-down capture instead of shipping the bare screen. Set SKIP_CAPTIONS=1
# for the old full-bleed set.
#
# The renderer is Swift/CoreText, not `magick -annotate`, because the
# ImageMagick installed here is built WITHOUT freetype:
#
#     magick -version | grep Delegates    # no "freetype" in the list
#
# With no freetype, every text operator silently falls through to Ghostscript
# and the build dies with `sh: gs: command not found`. Installing ghostscript
# would NOT fix it properly — it only gets you the PostScript base 35 fonts,
# not SF Pro. CoreText draws the same system font the app draws, so the
# caption and the screen underneath it match.
CAPTIONS_ENABLED=0
RENDER_BIN=""
if [ "${SKIP_CAPTIONS:-}" != "1" ] && [ -f captions.txt ]; then
    command -v swiftc >/dev/null || {
        echo "Need swiftc for captions (install Xcode, or run SKIP_CAPTIONS=1)" >&2; exit 1; }
    RENDER_BIN="$(mktemp -d)/caption-render"
    echo "Compiling caption renderer..."
    swiftc -O -o "$RENDER_BIN" caption-render.swift
    CAPTIONS_ENABLED=1
    trap 'rm -rf "$(dirname "$RENDER_BIN")"' EXIT
fi

# Looks up "NN" in captions.txt. Prints "headline\tsubhead", or nothing when
# the screenshot has no caption assigned.
caption_for() { # $1 = basename without extension
    [ "$CAPTIONS_ENABLED" = "1" ] || return 0
    awk -F'|' -v want="$1" '
        /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
        $1 == want { printf "%s\t%s", $2, $3; exit }
    ' captions.txt
}

# ── Canvas fitting ───────────────────────────────────────────────────────────

# Scale to fill, then crop to the exact canvas. Used for the iPhone sizes,
# whose aspect ratios are within a percent of the source — the crop removes a
# handful of pixels, not content. Uncaptioned path only.
fill_crop() { # $1 src, $2 WxH, $3 out
    magick "$1" -resize "${2}^" -gravity center -extent "$2" \
        -background "$PAD" -alpha remove -alpha off "$3"
}

# Scale to fit entirely, then pad. Used for iPad: cropping a phone shot to an
# iPad's aspect would cut off the bottom third of the UI, which on the form
# screens is most of the form. Uncaptioned path only.
fit_pad() { # $1 src, $2 WxH, $3 out
    magick "$1" -resize "$2" -gravity center -background "$PAD" -extent "$2" \
        -alpha remove -alpha off "$3"
}

# Captioned equivalent of both: the renderer fits the capture into whatever
# room the headline leaves, on every canvas aspect, so there's no separate
# crop-vs-pad decision to make. The magick pass after it is belt-and-braces on
# the alpha channel — CGImageDestination has been known to write RGBA even
# from a noneSkipLast context, and App Store Connect rejects that.
caption_fit() { # $1 src, $2 W, $3 H, $4 headline, $5 subhead, $6 out
    "$RENDER_BIN" "$1" "$2" "$3" "$4" "$5" "$6"
    magick "$6" -background "$PAD" -alpha remove -alpha off "$6"
}

render() { # $1 src, $2 W, $3 H, $4 out, $5 headline, $6 subhead, $7 fit-mode
    if [ -n "$5" ]; then
        caption_fit "$1" "$2" "$3" "$5" "$6" "$4"
    elif [ "$7" = "pad" ]; then
        fit_pad "$1" "${2}x${3}" "$4"
    else
        fill_crop "$1" "${2}x${3}" "$4"
    fi
}

for src in "${originals[@]}"; do
    n=$(basename "$src")
    stem="${n%.png}"

    IFS=$'\t' read -r headline subhead <<<"$(caption_for "$stem")"
    headline="${headline:-}"; subhead="${subhead:-}"

    # Mask on a copy so originals/ stays exactly as imported.
    work=$(mktemp -t shot).png
    cp "$src" "$work"
    mask_testflight "$work" "$n"
    src="$work"

    render "$src" 1206 2622 "iphone-6.3/$n" "$headline" "$subhead" crop
    render "$src" 1320 2868 "iphone-6.9/$n" "$headline" "$subhead" crop
    render "$src" 1284 2778 "iphone-6.5/$n" "$headline" "$subhead" crop
    render "$src" 2064 2752 "ipad-13/$n"    "$headline" "$subhead" pad
    rm -f "$work"

    if [ -n "$headline" ]; then
        echo "  $n -> 6.3, 6.9, 6.5, ipad-13   \"$headline\""
    else
        echo "  $n -> 6.3, 6.9, 6.5, ipad-13   (no caption)"
    fi
done

echo
echo "Verifying:"
for d in iphone-6.3 iphone-6.9 iphone-6.5 ipad-13; do
    for f in "$d"/*.png; do
        printf '  %-22s %s %s\n' "$f" \
            "$(magick identify -format '%wx%h' "$f")" \
            "$(magick identify -format '%[channels]' "$f")"
    done
done
