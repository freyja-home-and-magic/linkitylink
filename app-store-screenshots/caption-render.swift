// Renders one captioned App Store screenshot: headline + subhead above the
// device capture, on the app's dark ground.
//
//   caption-render <shot.png> <W> <H> <headline> <subhead> <out.png>
//
// Why CoreText rather than ImageMagick: the ImageMagick on this machine is
// built WITHOUT freetype (check `magick -version | grep Delegates`), so every
// -annotate/-draw text falls through to Ghostscript and fails outright. This
// also renders in the same SF Pro the app itself uses, which ImageMagick
// would not have picked up from the .ttc collections anyway.

import AppKit
import CoreGraphics
import CoreText
import Foundation

func die(_ msg: String) -> Never {
    FileHandle.standardError.write(("caption-render: " + msg + "\n").data(using: .utf8)!)
    exit(1)
}

let a = CommandLine.arguments
guard a.count == 7, let W = Int(a[2]), let H = Int(a[3]) else {
    die("usage: caption-render <shot.png> <W> <H> <headline> <subhead> <out.png>")
}
let shotPath = a[1], headline = a[4], subhead = a[5], outPath = a[6]

guard let isrc = CGImageSourceCreateWithURL(URL(fileURLWithPath: shotPath) as CFURL, nil),
      let shot = CGImageSourceCreateImageAtIndex(isrc, 0, nil) else {
    die("cannot read \(shotPath)")
}

let cw = CGFloat(W), ch = CGFloat(H)

// Text scales with the SHORTER relative dimension. Scaling on width alone
// would blow the type up on the iPad canvas (2064 wide), where the portrait
// capture is still the height-limited element and the type would dwarf it.
let scale = min(cw / 1320.0, ch / 2868.0)

guard let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8,
                          bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                          bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
    die("cannot create \(W)x\(H) context")
}

func rgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255.0,
            green: CGFloat((hex >> 8) & 0xff) / 255.0,
            blue: CGFloat(hex & 0xff) / 255.0, alpha: alpha)
}

// Palette straight out of app/src/style.css :root — glacier-white on the
// dark-mode ground, soft-mint for the supporting line.
let ink = rgb(0xF7F9FA)
let mint = rgb(0xAEE1D6)
let groundTop = rgb(0x1E2A26)   // --bg with a trace of --deep-evergreen
let groundBot = rgb(0x141414)

// ── background ───────────────────────────────────────────────────────────────
if let g = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                      colors: [groundTop, groundBot] as CFArray, locations: [0, 1]) {
    ctx.drawLinearGradient(g, start: CGPoint(x: 0, y: ch), end: CGPoint(x: 0, y: 0), options: [])
} else {
    ctx.setFillColor(groundBot); ctx.fill(CGRect(x: 0, y: 0, width: cw, height: ch))
}

// ── text ─────────────────────────────────────────────────────────────────────
// topY is measured DOWN from the top of the canvas; CoreText works in the
// context's bottom-left origin, so the rect is flipped on the way in.
func draw(_ text: String, size: CGFloat, weight: NSFont.Weight, color: CGColor,
          kern: CGFloat, maxWidth: CGFloat, topY: CGFloat) -> CGFloat {
    if text.isEmpty { return 0 }
    let para = NSMutableParagraphStyle()
    para.alignment = .center
    para.lineBreakMode = .byWordWrapping
    para.lineHeightMultiple = 1.08
    let attr = NSAttributedString(string: text, attributes: [
        .font: NSFont.systemFont(ofSize: size, weight: weight),
        .foregroundColor: NSColor(cgColor: color)!,
        .kern: kern,
        .paragraphStyle: para,
    ])
    let fs = CTFramesetterCreateWithAttributedString(attr)
    var fit = CFRange()
    let sz = CTFramesetterSuggestFrameSizeWithConstraints(
        fs, CFRange(location: 0, length: 0), nil,
        CGSize(width: maxWidth, height: .greatestFiniteMagnitude), &fit)
    // +6: the suggested size rounds down often enough to clip a descender.
    let h = ceil(sz.height) + 6
    let rect = CGRect(x: (cw - maxWidth) / 2, y: ch - topY - h, width: maxWidth, height: h)
    CTFrameDraw(CTFramesetterCreateFrame(fs, CFRange(location: 0, length: 0),
                                         CGPath(rect: rect, transform: nil), nil), ctx)
    return h
}

let textWidth = min(cw * 0.86, 1120 * scale)
let topPad = 132 * scale
var y = topPad
y += draw(headline, size: 76 * scale, weight: .bold, color: ink,
          kern: -1.0 * scale, maxWidth: textWidth, topY: y)
if !subhead.isEmpty {
    y += 22 * scale
    y += draw(subhead, size: 40 * scale, weight: .regular, color: mint,
              kern: 0, maxWidth: textWidth, topY: y)
}

// ── device capture ───────────────────────────────────────────────────────────
// Fit entirely, never crop: on the form screens the bottom of the capture is
// the Save button, and bleeding it off the canvas edge would cut it away.
let shotTop = y + 74 * scale
let bottomPad = 72 * scale
let boxW = min(cw - 2 * (92 * scale), 1100 * scale)
let boxH = ch - shotTop - bottomPad
let aspect = CGFloat(shot.width) / CGFloat(shot.height)
var sw = boxW, sh = boxW / aspect
if sh > boxH { sh = boxH; sw = boxH * aspect }
let sx = (cw - sw) / 2
let sy = ch - shotTop - ((boxH - sh) / 2) - sh   // centered in the leftover box
let frame = CGRect(x: sx, y: sy, width: sw, height: sh)
let radius = sw * 0.085

let rounded = CGPath(roundedRect: frame, cornerWidth: radius, cornerHeight: radius, transform: nil)

ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -16 * scale), blur: 44 * scale,
              color: rgb(0x000000, 0.55))
ctx.setFillColor(rgb(0x1a1a1a))
ctx.addPath(rounded); ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(rounded); ctx.clip()
ctx.draw(shot, in: frame)
ctx.restoreGState()

// --white-10 hairline, so the capture reads as a screen and not a hole.
ctx.saveGState()
ctx.setStrokeColor(rgb(0xF7F9FA, 0.10))
ctx.setLineWidth(2 * scale)
ctx.addPath(rounded); ctx.strokePath()
ctx.restoreGState()

// ── out ──────────────────────────────────────────────────────────────────────
guard let img = ctx.makeImage(),
      let dest = CGImageDestinationCreateWithURL(
          URL(fileURLWithPath: outPath) as CFURL, "public.png" as CFString, 1, nil) else {
    die("cannot write \(outPath)")
}
CGImageDestinationAddImage(dest, img, nil)
guard CGImageDestinationFinalize(dest) else { die("cannot finalize \(outPath)") }
