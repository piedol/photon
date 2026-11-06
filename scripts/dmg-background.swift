// Renders the disk image window's background: the app's accent colour toned to a mid grey, and an
// arrow from the app to the Applications folder. Mid-tone, because Finder draws the icon labels
// black in Light Mode and white in Dark Mode, and both have to stay readable.
// Shared with piedol's other Mac apps and kept the same in each.
// Usage: swift scripts/dmg-background.swift <accent, e.g. #0a84ff> <out@1x.png> <out@2x.png>
import AppKit

// The window is 640 × 400 points; the icons are centred at x 170 and 470, y 190 from the top
// (see scripts/dmg-settings.py).
let size = CGSize(width: 640, height: 400)
let iconY: CGFloat = 190

let args = CommandLine.arguments
guard args.count == 4, let accent = rgb(args[1]) else {
    FileHandle.standardError.write("usage: dmg-background.swift <#rrggbb> <out@1x.png> <out@2x.png>\n".data(using: .utf8)!)
    exit(2)
}

func rgb(_ hex: String) -> (CGFloat, CGFloat, CGFloat)? {
    let h = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
    guard h.count == 6, let v = UInt32(h, radix: 16) else { return nil }
    return (CGFloat((v >> 16) & 0xff) / 255, CGFloat((v >> 8) & 0xff) / 255, CGFloat(v & 0xff) / 255)
}

func linear(_ c: CGFloat) -> CGFloat { c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4) }
func luminance(_ c: (CGFloat, CGFloat, CGFloat)) -> CGFloat {
    0.2126 * linear(c.0) + 0.7152 * linear(c.1) + 0.0722 * linear(c.2)
}

/// The accent, half mixed with grey, scaled until its relative luminance is `target`.
func tone(_ target: CGFloat) -> NSColor {
    let base = ((accent.0 + 0.5) / 2, (accent.1 + 0.5) / 2, (accent.2 + 0.5) / 2)
    var lo: CGFloat = 0, hi: CGFloat = 3
    for _ in 0..<40 {
        let k = (lo + hi) / 2
        let c = (min(base.0 * k, 1), min(base.1 * k, 1), min(base.2 * k, 1))
        if luminance(c) < target { lo = k } else { hi = k }
    }
    return NSColor(srgbRed: min(base.0 * lo, 1), green: min(base.1 * lo, 1), blue: min(base.2 * lo, 1), alpha: 1)
}

func render(scale: CGFloat, to path: String) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    // A point size smaller than the pixel size marks the image as Retina and scales the drawing.
    rep.size = size
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    let space = CGColorSpace(name: CGColorSpace.sRGB)!

    // Luminance about 0.2: labels in black or white both stay above 4:1 contrast.
    let bg = CGGradient(colorsSpace: space, colors: [tone(0.23).cgColor, tone(0.18).cgColor] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(bg, start: CGPoint(x: 0, y: size.height), end: CGPoint(x: 0, y: 0), options: [])
    // A soft light behind the two icons.
    let glow = CGGradient(colorsSpace: space, colors: [NSColor(white: 1, alpha: 0.07).cgColor, NSColor(white: 1, alpha: 0).cgColor] as CFArray,
                          locations: [0, 1])!
    let centre = CGPoint(x: size.width / 2, y: size.height - iconY)
    ctx.drawRadialGradient(glow, startCenter: centre, startRadius: 0, endCenter: centre, endRadius: 300, options: [])

    // The arrow.
    let y = size.height - iconY
    let arrow = CGMutablePath()
    arrow.move(to: CGPoint(x: 262, y: y))
    arrow.addLine(to: CGPoint(x: 378, y: y))
    arrow.move(to: CGPoint(x: 360, y: y + 18))
    arrow.addLine(to: CGPoint(x: 378, y: y))
    arrow.addLine(to: CGPoint(x: 360, y: y - 18))
    ctx.setShadow(offset: CGSize(width: 0, height: -1), blur: 4, color: NSColor(white: 0, alpha: 0.25).cgColor)
    ctx.addPath(arrow)
    ctx.setStrokeColor(NSColor(white: 1, alpha: 0.9).cgColor)
    ctx.setLineWidth(6)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.strokePath()

    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

render(scale: 1, to: args[2])
render(scale: 2, to: args[3])
