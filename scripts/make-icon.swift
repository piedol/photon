// Renders assets/AppIcon.icns: a dark squircle, white crop brackets, a glowing photon.
// Usage: swift scripts/make-icon.swift
import AppKit

func draw(_ px: CGFloat) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(px), pixelsHigh: Int(px), bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = px / 1024
    let ctx = NSGraphicsContext.current!.cgContext
    ctx.scaleBy(x: s, y: s)

    // Squircle body (macOS icon grid: 824pt body centred in 1024).
    let body = CGRect(x: 100, y: 100, width: 824, height: 824)
    let path = CGPath(roundedRect: body, cornerWidth: 186, cornerHeight: 186, transform: nil)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 28, color: NSColor(white: 0, alpha: 0.35).cgColor)
    ctx.addPath(path); ctx.setFillColor(NSColor(srgbRed: 0.08, green: 0.08, blue: 0.10, alpha: 1).cgColor); ctx.fillPath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.addPath(path); ctx.clip()
    let bg = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [
        NSColor(srgbRed: 0.17, green: 0.17, blue: 0.21, alpha: 1).cgColor,
        NSColor(srgbRed: 0.06, green: 0.06, blue: 0.08, alpha: 1).cgColor] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(bg, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])

    // Photon glow.
    let c = CGPoint(x: 512, y: 512)
    let glow = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [
        NSColor(srgbRed: 0.55, green: 0.75, blue: 1.0, alpha: 0.55).cgColor,
        NSColor(srgbRed: 0.35, green: 0.45, blue: 1.0, alpha: 0.0).cgColor] as CFArray, locations: [0, 1])!
    ctx.drawRadialGradient(glow, startCenter: c, startRadius: 0, endCenter: c, endRadius: 260, options: [])
    ctx.setFillColor(NSColor(srgbRed: 0.93, green: 0.97, blue: 1.0, alpha: 1).cgColor)
    ctx.fillEllipse(in: CGRect(x: c.x - 46, y: c.y - 46, width: 92, height: 92))

    // Crop brackets.
    ctx.setStrokeColor(NSColor(white: 1, alpha: 0.95).cgColor)
    ctx.setLineWidth(44); ctx.setLineCap(.round); ctx.setLineJoin(.round)
    let (lo, hi, arm): (CGFloat, CGFloat, CGFloat) = (262, 762, 150)
    for (x, y, dx, dy) in [(lo, hi, 1.0, -1.0), (hi, hi, -1.0, -1.0), (lo, lo, 1.0, 1.0), (hi, lo, -1.0, 1.0)] as [(CGFloat, CGFloat, CGFloat, CGFloat)] {
        ctx.move(to: CGPoint(x: x, y: y + dy * arm)); ctx.addLine(to: CGPoint(x: x, y: y)); ctx.addLine(to: CGPoint(x: x + dx * arm, y: y))
        ctx.strokePath()
    }
    ctx.restoreGState()
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let set = URL(fileURLWithPath: "assets/AppIcon.iconset")
try? FileManager.default.removeItem(at: set)
try! FileManager.default.createDirectory(at: set, withIntermediateDirectories: true)
for (pt, scale) in [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)] {
    let name = scale == 1 ? "icon_\(pt)x\(pt).png" : "icon_\(pt)x\(pt)@2x.png"
    let data = draw(CGFloat(pt * scale)).representation(using: .png, properties: [:])!
    try! data.write(to: set.appendingPathComponent(name))
}
let p = Process(); p.launchPath = "/usr/bin/iconutil"; p.arguments = ["-c", "icns", set.path, "-o", "assets/AppIcon.icns"]
p.launch(); p.waitUntilExit()
try? FileManager.default.removeItem(at: set)
print("assets/AppIcon.icns")
