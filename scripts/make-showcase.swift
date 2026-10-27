// Composes the README showcase (docs/images/showcase.png, 2400 × 1480) and the GitHub social
// preview (docs/images/social-preview.png, 1280 × 640) from real screenshots in docs/images:
// the capture overlay with markup, the recording countdown and the "Recording saved" panel,
// on a graphite card with Photon's blue glow. A screenshot that hasn't been taken yet is drawn
// as a labelled placeholder of the size it will have. overlay.png comes from
// `photon://selftest-showcase` over scripts/showcase-backdrop.swift.
// Usage: swift scripts/make-showcase.swift [repo folder] [output folder]
import AppKit
import zlib

let appName = "Photon"
let blue = RGB(r: 0.04, g: 0.52, b: 1.0)   // #0A84FF, the system accent Photon's UI uses
// Expected sizes of the 2x captures (pixels), for the placeholders and the layout.
let overlay = CGSize(width: 1456, height: 1064)   // a full-screen capture, cropped to the selection and its bars
let countdown = CGSize(width: 232, height: 272)   // the countdown panel, a window capture
let saved = CGSize(width: 600, height: 176)       // recording.png: the "Recording saved" panel, a window capture

// MARK: - Family toolkit
// The same block sits in Photon's, Tamka's, Kipimo's and Kivuko's make-showcase.swift,
// so the four images stay siblings. Change it in all four.

let args = CommandLine.arguments
let root = (args.count > 1 ? args[1] : ".") + "/"
let outDir = (args.count > 2 ? args[2] : root + "docs/images") + "/"

struct RGB { var r, g, b: Double }
let graphiteLight = RGB(r: 0.15, g: 0.15, b: 0.19), graphiteDark = RGB(r: 0.05, g: 0.05, b: 0.07)
func cg(_ c: RGB, _ a: CGFloat = 1) -> CGColor { CGColor(srgbRed: c.r, green: c.g, blue: c.b, alpha: a) }

/// Every screenshot is a 2x capture; one scale per image keeps all their text the same size.
let S: CGFloat = 0.88

/// A soft pool of accent light, centred at (x, top) in top-left coordinates.
struct Glow { var x: Double; var top: Double; var radius: Double; var alpha: Double }

func image(_ path: String) -> NSImage? { NSImage(contentsOfFile: root + path) }
func pixels(_ img: NSImage) -> CGSize {
    let rep = img.representations.max { $0.pixelsWide < $1.pixelsWide }!
    return CGSize(width: rep.pixelsWide, height: rep.pixelsHigh)
}
/// The app icon: assets/logo.png, else the 1024 px image inside assets/AppIcon.icns.
func appIcon() -> NSImage? { image("assets/logo.png") ?? image("assets/AppIcon.icns") }

final class Canvas {
    let width: Int, height: Int
    let ctx: CGContext
    let opaque: Bool
    var W: CGFloat { CGFloat(width) }
    var H: CGFloat { CGFloat(height) }

    /// A graphite backdrop lit by `glows` in the app's accent: a card with rounded,
    /// transparent corners when `corner` > 0, else full bleed and opaque. It's painted pixel
    /// by pixel with an 8×8 ordered dither, which hides banding in the dark gradient and
    /// compresses far better than the random dither of a CGGradient.
    init(_ w: Int, _ h: Int, corner: Double, accent: RGB, glows: [Glow]) {
        width = w; height = h; opaque = corner == 0
        ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let px = ctx.data!.assumingMemoryBound(to: UInt8.self)
        let bayer: [Double] = [0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26,
                               12, 44, 4, 36, 14, 46, 6, 38, 60, 28, 52, 20, 62, 30, 54, 22,
                               3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25,
                               15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21].map { ($0 + 0.5) / 64 }
        let (fw, fh) = (Double(w), Double(h))
        let diag = fw * fw + fh * fh
        let (a, b) = (graphiteLight, graphiteDark)
        for y in 0..<h {
            let fy = Double(y) + 0.5
            let row = px + y * w * 4
            for x in 0..<w {
                let fx = Double(x) + 0.5
                // Linear gradient from the top-left corner to the bottom-right one.
                let t = min(1, max(0, (fx * fw + fy * fh) / diag))
                var r = a.r + (b.r - a.r) * t, g = a.g + (b.g - a.g) * t, bl = a.b + (b.b - a.b) * t
                for glow in glows {
                    let dx = fx - glow.x, dy = fy - glow.top
                    let d2 = dx * dx + dy * dy
                    if d2 >= glow.radius * glow.radius { continue }
                    let k = glow.alpha * (1 - d2.squareRoot() / glow.radius)
                    r += (accent.r - r) * k; g += (accent.g - g) * k; bl += (accent.b - bl) * k
                }
                // Coverage of the rounded card at this pixel (1 inside, 0 outside the corners).
                var cover = 1.0
                if corner > 0 {
                    let qx = abs(fx - fw / 2) - (fw / 2 - corner), qy = abs(fy - fh / 2) - (fh / 2 - corner)
                    if qx > 0 && qy > 0 {
                        cover = min(1, max(0, 0.5 - ((qx * qx + qy * qy).squareRoot() - corner)))
                    }
                }
                let d = bayer[(y & 7) * 8 + (x & 7)]
                let p = row + x * 4
                p[0] = UInt8(min(255, (r * cover * 255 + d).rounded(.down)))
                p[1] = UInt8(min(255, (g * cover * 255 + d).rounded(.down)))
                p[2] = UInt8(min(255, (bl * cover * 255 + d).rounded(.down)))
                p[3] = UInt8(min(255, (cover * 255).rounded()))
            }
        }
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
        ctx.interpolationQuality = .high
        if corner > 0 {
            ctx.addPath(CGPath(roundedRect: CGRect(x: 0, y: 0, width: w, height: h),
                               cornerWidth: corner, cornerHeight: corner, transform: nil))
            ctx.clip()
        }
    }

    /// A rect given by its top-left corner, in top-left coordinates.
    func rect(_ x: CGFloat, _ top: CGFloat, _ w: CGFloat, _ h: CGFloat) -> CGRect {
        CGRect(x: x, y: H - top - h, width: w, height: h)
    }

    /// Draw `img` `width` pixels wide with its top-left at (x, top), with a soft drop shadow.
    /// Window captures bring their own rounded corners; `corner` rounds a rectangular crop
    /// of the screen to match.
    @discardableResult
    func place(_ img: NSImage, x: CGFloat, top: CGFloat, width: CGFloat, shadow: Bool = true, corner: CGFloat = 0) -> CGRect {
        let px = pixels(img)
        let r = rect(x, top, width, width * px.height / px.width)
        let k = width / px.width / S
        ctx.saveGState()
        if shadow {
            ctx.setShadow(offset: CGSize(width: 0, height: -18 * k), blur: 60 * k, color: CGColor(gray: 0, alpha: 0.55))
        }
        if corner > 0 {
            let path = CGPath(roundedRect: r, cornerWidth: corner, cornerHeight: corner, transform: nil)
            ctx.addPath(path); ctx.setFillColor(CGColor(gray: 0, alpha: 1)); ctx.fillPath()   // casts the shadow
            ctx.restoreGState(); ctx.saveGState()
            ctx.addPath(path); ctx.clip()
        }
        img.draw(in: r, from: .zero, operation: .sourceOver, fraction: 1)
        ctx.restoreGState()
        return r
    }

    /// A labelled, dashed box the size a missing screenshot will have.
    func placeholder(_ name: String, _ note: String, _ r: CGRect, scale: CGFloat) {
        let path = CGPath(roundedRect: r, cornerWidth: 22 * scale / S, cornerHeight: 22 * scale / S, transform: nil)
        ctx.saveGState()
        ctx.addPath(path); ctx.setFillColor(CGColor(gray: 1, alpha: 0.04)); ctx.fillPath()
        ctx.addPath(path); ctx.setStrokeColor(CGColor(gray: 1, alpha: 0.32)); ctx.setLineWidth(3 * scale / S)
        ctx.setLineDash(phase: 0, lengths: [18 * scale / S, 12 * scale / S]); ctx.strokePath()
        ctx.restoreGState()
        let k = max(0.55, min(1.4, min(r.width, r.height) / 420))
        let title = label(name, size: 36 * k, weight: .semibold, alpha: 0.75)
        let sub = label(note, size: 24 * k, weight: .regular, alpha: 0.5)
        let gap = 14 * k
        let total = title.size().height + gap + sub.size().height
        let y0 = r.midY - total / 2
        sub.draw(at: CGPoint(x: r.midX - sub.size().width / 2, y: y0))
        title.draw(at: CGPoint(x: r.midX - title.size().width / 2, y: y0 + sub.size().height + gap))
    }

    /// A screenshot from docs/images at `scale`, or a placeholder of the size it will have
    /// (`expect`, in pixels of the 2x capture) while it hasn't been taken yet. `corner` is in
    /// capture pixels.
    /// Position it by its left edge (`x`), centre (`cx`) or right edge (`right`).
    @discardableResult
    func shot(_ name: String, expect: CGSize, x: CGFloat? = nil, cx: CGFloat? = nil, right: CGFloat? = nil,
              top: CGFloat, scale: CGFloat = S, corner: CGFloat = 0) -> CGRect {
        let img = image("docs/images/" + name)
        let size = img.map(pixels) ?? expect
        let (w, h) = (size.width * scale, size.height * scale)
        let left = x ?? cx.map { $0 - w / 2 } ?? (right! - w)
        if let img { return place(img, x: left, top: top, width: w, corner: corner * scale) }
        let r = rect(left, top, w, h)
        placeholder(name, "capture pending · \(Int(expect.width)) × \(Int(expect.height)) px", r, scale: scale)
        return r
    }
    func size(_ name: String, expect: CGSize, scale: CGFloat = S) -> CGSize {
        let s = image("docs/images/" + name).map(pixels) ?? expect
        return CGSize(width: s.width * scale, height: s.height * scale)
    }

    /// The name block: icon beside the app's name and a line or two of copy. The type scales
    /// with the icon (168 px in a showcase). Returns its frame.
    /// `copy` sets the size of the lines under the name (40 pt per 168 px of icon by default).
    @discardableResult
    func nameBlock(_ name: String, _ lines: [String], x: CGFloat? = nil, cx: CGFloat? = nil, top: CGFloat,
                   icon: CGFloat = 168, copy: CGFloat? = nil) -> CGRect {
        let k = icon / 168
        let c = copy ?? 40 * k
        let title = label(name, size: 104 * k, weight: .bold, alpha: 1)
        let text = lines.map { label($0, size: c, weight: .medium, alpha: 0.62) }
        let gap = 34 * k
        let textW = ([title] + text).map { $0.size().width }.max()!
        let left = x ?? (cx! - (icon + gap + textW) / 2)
        let textX = left + icon + gap
        // Bottoms of the name's and each line's boxes, measured from `top`.
        let nameBottom = 112 * k
        title.draw(at: CGPoint(x: textX, y: H - top - nameBottom))
        var bottom = nameBottom
        for (i, line) in text.enumerated() {
            bottom = nameBottom + c * (54 + 50 * CGFloat(i)) / 40
            line.draw(at: CGPoint(x: textX, y: H - top - bottom))
        }
        // The icon sits level with the text, or centred on it when the text is taller.
        let iconTop = top + max(0, (bottom - icon) / 2)
        if let logo = appIcon() { place(logo, x: left, top: iconTop, width: icon, shadow: false) }
        return rect(left, top, icon + gap + textW, max(icon, bottom))
    }

    /// The width `nameBlock` will take, for centring it with other pieces.
    func nameBlockWidth(_ name: String, _ lines: [String], icon: CGFloat = 168, copy: CGFloat? = nil) -> CGFloat {
        let k = icon / 168
        let widths = [label(name, size: 104 * k, weight: .bold, alpha: 1).size().width] +
            lines.map { label($0, size: copy ?? 40 * k, weight: .medium, alpha: 1).size().width }
        return icon + 34 * k + widths.max()!
    }

    func label(_ s: String, size: CGFloat, weight: NSFont.Weight, alpha: CGFloat) -> NSAttributedString {
        NSAttributedString(string: s, attributes: [.font: NSFont.systemFont(ofSize: size, weight: weight),
                                                   .foregroundColor: NSColor(white: 1, alpha: alpha)])
    }

    /// Write the image as a PNG and return its size in bytes. The encoder is ImageIO-free so
    /// it can choose a filter per row (as libpng's heuristic does) and use zlib's best
    /// compression; ImageIO's PNGs of the same pixels come out about 40% larger.
    @discardableResult
    func write(_ file: String) -> Int {
        let data = encodePNG(ctx, opaque: opaque)
        let url = URL(fileURLWithPath: outDir + file)
        try! FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try! data.write(to: url)
        print("Wrote \(outDir + file): \(width) × \(height), \(data.count / 1024) KB")
        return data.count
    }
}

/// A lossless PNG (8-bit RGBA, or RGB when opaque) of the context's pixels, tagged sRGB.
func encodePNG(_ ctx: CGContext, opaque: Bool) -> Data {
    let (w, h, bpr) = (ctx.width, ctx.height, ctx.bytesPerRow)
    let src = ctx.data!.assumingMemoryBound(to: UInt8.self)
    let ch = opaque ? 3 : 4
    let rowLen = w * ch
    let raw = UnsafeMutablePointer<UInt8>.allocate(capacity: (rowLen + 1) * h)
    let prev = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLen)
    let cur = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLen)
    let trial = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLen)
    let best = UnsafeMutablePointer<UInt8>.allocate(capacity: rowLen)
    prev.initialize(repeating: 0, count: rowLen)
    defer { [raw, prev, cur, trial, best].forEach { $0.deallocate() } }
    for y in 0..<h {
        // Straight (not premultiplied) colour, as PNG stores it.
        let s = src + y * bpr
        for x in 0..<w {
            let p = s + x * 4, o = cur + x * ch
            let a = Int(p[3])
            if a == 255 || a == 0 {
                o[0] = p[0]; o[1] = p[1]; o[2] = p[2]
            } else {
                o[0] = UInt8(min(255, (Int(p[0]) * 255 + a / 2) / a))
                o[1] = UInt8(min(255, (Int(p[1]) * 255 + a / 2) / a))
                o[2] = UInt8(min(255, (Int(p[2]) * 255 + a / 2) / a))
            }
            if !opaque { o[3] = p[3] }
        }
        // Try the five filters; keep the one with the smallest sum of signed residuals.
        var bestScore = Int.max, bestType: UInt8 = 0
        for type in 0..<5 {
            var score = 0
            for i in 0..<rowLen {
                let left = i >= ch ? Int(cur[i - ch]) : 0
                let up = Int(prev[i])
                let ul = i >= ch ? Int(prev[i - ch]) : 0
                let pred: Int
                switch type {
                case 0: pred = 0
                case 1: pred = left
                case 2: pred = up
                case 3: pred = (left + up) / 2
                default:
                    let pa = abs(up - ul), pb = abs(left - ul), pc = abs(left + up - 2 * ul)
                    pred = pa <= pb && pa <= pc ? left : (pb <= pc ? up : ul)
                }
                let v = UInt8(truncatingIfNeeded: Int(cur[i]) - pred)
                trial[i] = v
                score += v < 128 ? Int(v) : 256 - Int(v)
            }
            if score < bestScore { bestScore = score; bestType = UInt8(type); best.update(from: trial, count: rowLen) }
        }
        let o = raw + y * (rowLen + 1)
        o[0] = bestType
        (o + 1).update(from: best, count: rowLen)
        prev.update(from: cur, count: rowLen)
    }
    // zlib at its best compression.
    var z = z_stream()
    deflateInit2_(&z, 9, Z_DEFLATED, 15, 9, Z_DEFAULT_STRATEGY, zlibVersion(), Int32(MemoryLayout<z_stream>.size))
    let inLen = (rowLen + 1) * h
    let cap = Int(deflateBound(&z, uLong(inLen)))
    let out = UnsafeMutablePointer<UInt8>.allocate(capacity: cap)
    defer { out.deallocate() }
    z.next_in = raw; z.avail_in = uInt(inLen)
    z.next_out = out; z.avail_out = uInt(cap)
    deflate(&z, Z_FINISH)
    let idat = Data(bytes: out, count: Int(z.total_out))
    deflateEnd(&z)

    var png = Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
    func chunk(_ type: String, _ body: Data) {
        var len = UInt32(body.count).bigEndian
        png.append(Data(bytes: &len, count: 4))
        var c = Data(type.utf8); c.append(body)
        png.append(c)
        var crc = UInt32(c.withUnsafeBytes { crc32(0, $0.bindMemory(to: UInt8.self).baseAddress, uInt(c.count)) }).bigEndian
        png.append(Data(bytes: &crc, count: 4))
    }
    var ihdr = Data()
    for v in [UInt32(w), UInt32(h)] { var be = v.bigEndian; ihdr.append(Data(bytes: &be, count: 4)) }
    ihdr.append(contentsOf: [8, opaque ? 2 : 6, 0, 0, 0])
    chunk("IHDR", ihdr)
    chunk("sRGB", Data([0]))
    chunk("IDAT", idat)
    chunk("IEND", Data())
    return png
}

// MARK: - Showcase: the overlay, marked up, on the right; on the left the name above the two
// panels a recording shows, the countdown and "Recording saved".
do {
    let (W, H): (CGFloat, CGFloat) = (2400, 1480)
    let margin: CGFloat = 120
    let o = CGSize(width: overlay.width * S, height: overlay.height * S)
    let ox = W - margin - o.width, oTop = (H - o.height) / 2
    let c = Canvas(Int(W), Int(H), corner: 56, accent: blue,
                   glows: [Glow(x: Double(ox + o.width * 0.5), top: Double(oTop + o.height * 0.55), radius: 900, alpha: 0.20),
                           Glow(x: Double(margin + 300), top: Double(oTop + o.height - 120), radius: 520, alpha: 0.10)])
    c.nameBlock(appName, ["Screenshots, markup", "and screen recording"], x: margin, top: oTop + 10)
    let cd = c.size("countdown.png", expect: countdown)
    let sv = c.size("recording.png", expect: saved)
    let bottom = oTop + o.height
    c.shot("countdown.png", expect: countdown, x: margin, top: bottom - cd.height)
    c.shot("recording.png", expect: saved, x: margin + cd.width + 48, top: bottom - sv.height)
    c.shot("overlay.png", expect: overlay, x: ox, top: oTop, corner: 24)
    c.write("showcase.png")
}

// MARK: - Social preview: name and platforms on the left; the overlay on the right.
// Everything that matters stays inside x 96…1184, y 56…584, since some sites crop the edges.
do {
    let s: CGFloat = 0.34
    let o = CGSize(width: overlay.width * s, height: overlay.height * s)
    let ox: CGFloat = 1184 - o.width, top = (640 - o.height) / 2
    let c = Canvas(1280, 640, corner: 0, accent: blue,
                   glows: [Glow(x: Double(ox + o.width / 2), top: Double(top + o.height * 0.55), radius: 520, alpha: 0.24)])
    let block = c.nameBlock(appName, ["Screenshots, markup", "and screen recording"], x: 96, top: 196, icon: 128, copy: 40)
    let facts = c.label("For Mac and Windows · under 1 MB", size: 24, weight: .medium, alpha: 0.45)
    facts.draw(at: CGPoint(x: block.minX + 128 + 34 * 128 / 168, y: block.minY - 22 - facts.size().height))
    c.shot("overlay.png", expect: overlay, x: ox, top: top, scale: s, corner: 24)
    c.write("social-preview.png")
}
