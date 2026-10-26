// Stages a neutral screen for the README's overlay shot (docs/images/overlay.png), so the
// frozen screen Photon marks up holds nothing of anyone's: a dusk-blue wall over the whole
// primary display, menu bar and Dock included, and one real document window, "Weekly
// Signups", whose white page holds made-up figures and example.com addresses.
// `photon://selftest-showcase` (PHOTON_DEV=1) selects and marks up this page; both use the
// geometry below, so keep them in step.
//
// Usage: swift scripts/showcase-backdrop.swift [seconds]          show it (default 180 s, or Ctrl-C)
//        swift scripts/showcase-backdrop.swift --preview page.png [--plan]
//                                     render only the page at 2x; --plan outlines the
//                                     selection and where the session draws, for checking
import AppKit

/// The page (the window's content), centred on the primary display.
let page = CGSize(width: 820, height: 590)
/// What the session selects, in page points from the page's top-left corner.
let selection = CGRect(x: 80, y: 80, width: 640, height: 420)
/// What the session saves: the selection with its size badge above, tool palette to the right
/// and action bar below. Only blank page lies between the selection and this edge, so the crop
/// never cuts through text.
let crop = CGRect(x: 56, y: 32, width: 728, height: 532)

func colour(_ hex: UInt32) -> NSColor {
    NSColor(srgbRed: CGFloat(hex >> 16 & 0xff) / 255, green: CGFloat(hex >> 8 & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: 1)
}
let ink = colour(0x1C1C1E), grey = colour(0x8E8E93), card = colour(0xF2F2F7)
let blue = colour(0x0A84FF), paleBlue = colour(0xA7CCFF), green = colour(0x248A3D)

func text(_ s: String, _ size: CGFloat, _ weight: NSFont.Weight, _ c: NSColor, at p: CGPoint, right: Bool = false) {
    let a = NSAttributedString(string: s, attributes: [.font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: c])
    a.draw(at: right ? CGPoint(x: p.x - a.size().width, y: p.y) : p)
}
func fill(_ r: CGRect, _ c: NSColor, radius: CGFloat = 0) {
    c.setFill()
    NSBezierPath(roundedRect: r, xRadius: radius, yRadius: radius).fill()
}

/// The page, in a flipped (top-left origin) context, in points.
func drawPage() {
    fill(CGRect(origin: .zero, size: page), .white)
    // Outside the crop: what makes the whole window look like a document.
    text("Growth · Weekly report", 12, .regular, grey, at: CGPoint(x: 24, y: 9))
    text("Updated 9:41", 12, .regular, grey, at: CGPoint(x: page.width - 24, y: 9), right: true)
    text("Signups rose after Tuesday’s newsletter; Wednesday and Thursday were the best days.",
         12, .regular, grey, at: CGPoint(x: 24, y: 568))

    let (sx, sy) = (selection.minX, selection.minY)
    text("Signups", 30, .semibold, ink, at: CGPoint(x: sx + 32, y: sy + 22))
    text("Week 38 · all regions", 14, .regular, grey, at: CGPoint(x: sx + 32, y: sy + 64))

    // Seven days of signups; Wednesday and Thursday, after the newsletter, are the best.
    let chart = CGRect(x: sx + 32, y: sy + 100, width: 360, height: 196)
    fill(chart, card, radius: 14)
    let heights: [CGFloat] = [70, 96, 150, 138, 88, 60, 52]
    let baseline = chart.maxY - 26
    for (i, h) in heights.enumerated() {
        let x = chart.minX + 21 + CGFloat(i) * 48
        fill(CGRect(x: x, y: baseline - h, width: 30, height: h), i == 2 || i == 3 ? blue : paleBlue, radius: 6)
        text(["M", "T", "W", "T", "F", "S", "S"][i], 11, .medium, grey, at: CGPoint(x: x + 10, y: baseline + 6))
    }

    let stat = CGRect(x: sx + 412, y: sy + 100, width: 196, height: 196)
    fill(stat, card, radius: 14)
    text("This week", 13, .regular, grey, at: CGPoint(x: stat.minX + 20, y: stat.minY + 22))
    text("12.4k", 40, .bold, ink, at: CGPoint(x: stat.minX + 18, y: stat.minY + 42))
    text("+38%", 17, .semibold, green, at: CGPoint(x: stat.minX + 20, y: stat.minY + 102))
    text("vs last week", 13, .regular, grey, at: CGPoint(x: stat.minX + 20, y: stat.minY + 126))

    text("Top referrers", 13, .semibold, grey, at: CGPoint(x: sx + 32, y: sy + 306))
    let rows: [(String, String, String, UInt32)] = [
        ("Avery Chen", "avery@example.com", "1,204", 0xFF9500),
        ("Sam Rivera", "sam@example.com", "982", 0xBF5AF2),
        ("Jordan Lee", "jordan@example.com", "871", 0x30D158),
    ]
    for (i, r) in rows.enumerated() {
        let y = sy + 332 + CGFloat(i) * 28
        colour(r.3).setFill()
        NSBezierPath(ovalIn: CGRect(x: sx + 32, y: y + 2, width: 20, height: 20)).fill()
        text(r.0, 15, .medium, ink, at: CGPoint(x: sx + 62, y: y + 2))
        text(r.1, 14, .regular, grey, at: CGPoint(x: sx + 180, y: y + 3))
        text(r.2, 15, .semibold, ink, at: CGPoint(x: sx + 392, y: y + 2), right: true)
    }

}

/// Where the session draws (see selftest-showcase), outlined for checking the geometry.
func drawPlan() {
    let red = NSColor.systemRed, yellow = NSColor.systemYellow
    func box(_ r: CGRect, _ c: NSColor, _ label: String) {
        c.setStroke()
        let p = NSBezierPath(rect: r); p.lineWidth = 1.5; p.setLineDash([5, 4], count: 2, phase: 0); p.stroke()
        text(label, 10, .semibold, c, at: CGPoint(x: r.minX + 2, y: r.minY - 13))
    }
    box(crop, .systemGray, "overlay.png crop 728 × 532 pt")
    box(selection, .systemBlue, "selection 640 × 420")
    box(CGRect(x: selection.maxX + 8, y: selection.maxY - 352, width: 36, height: 352), .systemGray, "palette")
    box(CGRect(x: selection.maxX - 184, y: selection.maxY + 8, width: 184, height: 36), .systemGray, "action bar")
    box(CGRect(x: selection.minX, y: selection.minY - 28, width: 96, height: 22), .systemGray, "badge")
    box(CGRect(x: 219, y: 188, width: 98, height: 170), red, "rect")
    box(CGRect(x: 205, y: 174, width: 28, height: 28), red, "step 1")
    box(CGRect(x: 520, y: 444, width: 150, height: 30), red, "text: Best days yet")
    let arrow = NSBezierPath(); arrow.move(to: CGPoint(x: 566, y: 438)); arrow.line(to: CGPoint(x: 324, y: 296))
    red.setStroke(); arrow.lineWidth = 1.5; arrow.stroke()
    box(CGRect(x: 138, y: 413, width: 98, height: 20), yellow, "marker")
    box(CGRect(x: 256, y: 438, width: 140, height: 24), red, "pixelate")
    box(CGRect(x: 400, y: 436, width: 28, height: 28), red, "step 2")
}

let args = CommandLine.arguments
if let i = args.firstIndex(of: "--preview"), i + 1 < args.count {
    let scale: CGFloat = 2
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(page.width * scale), pixelsHigh: Int(page.height * scale),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!.cgContext
    ctx.translateBy(x: 0, y: page.height * scale); ctx.scaleBy(x: scale, y: -scale)
    NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
    drawPage()
    if args.contains("--plan") { drawPlan() }
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[i + 1]))
    print("Wrote \(args[i + 1])")
    exit(0)
}

final class PageView: NSView {
    override var isFlipped: Bool { true }
    override func draw(_ dirtyRect: NSRect) { drawPage() }
}
final class WallView: NSView {
    override func draw(_ dirtyRect: NSRect) {
        NSGradient(starting: NSColor(srgbRed: 0.24, green: 0.31, blue: 0.45, alpha: 1),
                   ending: NSColor(srgbRed: 0.09, green: 0.11, blue: 0.19, alpha: 1))!.draw(in: bounds, angle: -60)
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.appearance = NSAppearance(named: .darkAqua)
let screen = NSScreen.screens[0].frame
// Above the menu bar and its status items; Photon's overlay sits higher still.
let wallLevel = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.statusWindow)) + 1)
let wall = NSWindow(contentRect: screen, styleMask: .borderless, backing: .buffered, defer: false)
wall.level = wallLevel
wall.contentView = WallView()
wall.ignoresMouseEvents = true
wall.collectionBehavior = [.canJoinAllSpaces, .stationary]
wall.orderFrontRegardless()

let content = CGRect(x: screen.midX - page.width / 2, y: screen.midY - page.height / 2, width: page.width, height: page.height)
let doc = NSWindow(contentRect: content, styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
doc.title = "Weekly Signups"
doc.level = NSWindow.Level(rawValue: wallLevel.rawValue + 1)
doc.contentView = PageView()
doc.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
print("page at x \(Int(content.minX)), y \(Int(content.minY)) (points, bottom-left origin); selection \(selection)")

let seconds = args.dropFirst().compactMap { Double($0) }.first ?? 180
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { app.terminate(nil) }
app.run()
