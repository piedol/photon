//! CoreGraphics drawing of annotation shapes. The same code draws on screen
//! (view points) and into the export bitmap (scaled to pixels), so output is sharp.

use objc2_app_kit::{
    NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext, NSColor,
    NSAttributedStringNSStringDrawing, NSFontWeightBold, NSFontWeightSemibold,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo, CGInterpolationQuality, CGLineCap, CGLineJoin, CGPathDrawingMode,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSString};
use objc2::runtime::AnyObject;
use objc2::rc::Retained;
use objc2::AnyThread;

use super::model::{Rgba, Shape};
use crate::capture::cg_rect;
use crate::geometry::{view_to_image_px, Point, Rect};

/// The frozen display image, needed by the pixelate tool.
pub struct Source<'a> {
    pub image: &'a CGImage,
    /// Height of the display in points (view space).
    pub view_h: f64,
    pub scale: f64,
}

fn c(ctx: &CGContext) -> Option<&CGContext> {
    Some(ctx)
}

fn set_stroke(ctx: &CGContext, col: Rgba) {
    CGContext::set_rgb_stroke_color(c(ctx), col.0, col.1, col.2, col.3);
}
fn set_fill(ctx: &CGContext, col: Rgba) {
    CGContext::set_rgb_fill_color(c(ctx), col.0, col.1, col.2, col.3);
}

fn smooth_path(ctx: &CGContext, pts: &[Point]) {
    let Some(first) = pts.first() else { return };
    CGContext::begin_path(c(ctx));
    CGContext::move_to_point(c(ctx), first.x, first.y);
    if pts.len() < 3 {
        for p in &pts[1..] {
            CGContext::add_line_to_point(c(ctx), p.x, p.y);
        }
        return;
    }
    // Quadratic curves through midpoints give smooth freehand strokes.
    for w in pts.windows(2).skip(1) {
        let (a, b) = (w[0], w[1]);
        CGContext::add_quad_curve_to_point(c(ctx), a.x, a.y, (a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    }
    let last = pts[pts.len() - 1];
    CGContext::add_line_to_point(c(ctx), last.x, last.y);
}

fn round_caps(ctx: &CGContext) {
    CGContext::set_line_cap(c(ctx), CGLineCap::Round);
    CGContext::set_line_join(c(ctx), CGLineJoin::Round);
}

/// Draw every shape. The context must already be in view-point space.
pub fn draw_shapes(ctx: &CGContext, shapes: &[Shape], src: &Source) {
    for s in shapes {
        draw_shape(ctx, s, src);
    }
}

pub fn draw_shape(ctx: &CGContext, shape: &Shape, src: &Source) {
    CGContext::save_g_state(c(ctx));
    match shape {
        Shape::Pen { pts, style } => {
            round_caps(ctx);
            CGContext::set_line_width(c(ctx), style.width());
            set_stroke(ctx, style.color);
            smooth_path(ctx, pts);
            CGContext::stroke_path(c(ctx));
        }
        Shape::Marker { pts, style } => {
            round_caps(ctx);
            CGContext::set_line_width(c(ctx), style.marker_width());
            let col = style.color;
            set_stroke(ctx, Rgba(col.0, col.1, col.2, 0.38));
            smooth_path(ctx, pts);
            CGContext::stroke_path(c(ctx));
        }
        Shape::Line { a, b, style } => {
            round_caps(ctx);
            CGContext::set_line_width(c(ctx), style.width());
            set_stroke(ctx, style.color);
            CGContext::begin_path(c(ctx));
            CGContext::move_to_point(c(ctx), a.x, a.y);
            CGContext::add_line_to_point(c(ctx), b.x, b.y);
            CGContext::stroke_path(c(ctx));
        }
        Shape::Arrow { a, b, style } => draw_arrow(ctx, *a, *b, style.width(), style.color),
        Shape::Rect { r, style } => {
            CGContext::set_line_join(c(ctx), CGLineJoin::Miter);
            CGContext::set_line_width(c(ctx), style.width());
            set_stroke(ctx, style.color);
            CGContext::stroke_rect(c(ctx), cg_rect(*r));
        }
        Shape::Ellipse { r, style } => {
            CGContext::set_line_width(c(ctx), style.width());
            set_stroke(ctx, style.color);
            CGContext::stroke_ellipse_in_rect(c(ctx), cg_rect(*r));
        }
        Shape::Text { origin, text, style } => draw_text(ctx, *origin, text, style.font_size(), style.color),
        Shape::Blur { r, style } => draw_pixelate(ctx, *r, style.blur_block(), src),
        Shape::Step { center, n, style } => draw_step(ctx, *center, *n, style.step_radius(), style.color),
    }
    CGContext::restore_g_state(c(ctx));
}

fn draw_arrow(ctx: &CGContext, a: Point, b: Point, w: f64, col: Rgba) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = (w * 4.5 + 6.0).min(len * 0.6);
    let half = head * 0.42;
    let base = Point { x: b.x - ux * head, y: b.y - uy * head };
    set_fill(ctx, col);
    set_stroke(ctx, col);
    round_caps(ctx);
    CGContext::set_line_width(c(ctx), w);
    CGContext::begin_path(c(ctx));
    CGContext::move_to_point(c(ctx), a.x, a.y);
    // Stop the shaft inside the head so the tip stays sharp.
    CGContext::add_line_to_point(c(ctx), base.x + ux * head * 0.3, base.y + uy * head * 0.3);
    CGContext::stroke_path(c(ctx));
    CGContext::set_line_width(c(ctx), 1.0);
    CGContext::set_line_join(c(ctx), CGLineJoin::Round);
    CGContext::begin_path(c(ctx));
    CGContext::move_to_point(c(ctx), b.x, b.y);
    CGContext::add_line_to_point(c(ctx), base.x - uy * half, base.y + ux * half);
    CGContext::add_line_to_point(c(ctx), base.x + ux * head * 0.12, base.y + uy * head * 0.12);
    CGContext::add_line_to_point(c(ctx), base.x + uy * half, base.y - ux * half);
    CGContext::close_path(c(ctx));
    CGContext::draw_path(c(ctx), CGPathDrawingMode::FillStroke);
}

fn ns_color(col: Rgba) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(col.0, col.1, col.2, col.3)
}

fn attributed(text: &str, font: &NSFont, col: Rgba) -> Retained<NSAttributedString> {
    let color = ns_color(col);
    let keys: [&NSString; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let vals: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
    let attrs = NSDictionary::from_slices(&keys, &vals);
    unsafe { NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(text), Some(&attrs)) }
}

pub fn text_font(size: f64) -> Retained<NSFont> {
    NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightSemibold })
}

/// Runs `f` with an AppKit graphics context wrapping `ctx` (needed for text).
fn with_ns_context(ctx: &CGContext, f: impl FnOnce()) {
    let ns = NSGraphicsContext::graphicsContextWithCGContext_flipped(ctx, false);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ns));
    f();
    NSGraphicsContext::restoreGraphicsState_class();
}

/// `origin` is the top-left of the first line.
fn draw_text(ctx: &CGContext, origin: Point, text: &str, size: f64, col: Rgba) {
    if text.is_empty() {
        return;
    }
    let font = text_font(size);
    let s = attributed(text, &font, col);
    let sz = s.size();
    with_ns_context(ctx, || {
        s.drawAtPoint(objc2_foundation::NSPoint::new(origin.x, origin.y - sz.height));
    });
}

fn draw_step(ctx: &CGContext, center: Point, n: u32, r: f64, col: Rgba) {
    let rect = CGRect::new(CGPoint::new(center.x - r, center.y - r), CGSize::new(r * 2.0, r * 2.0));
    CGContext::set_shadow_with_color(c(ctx), CGSize::new(0.0, -1.0), 3.0, None);
    set_fill(ctx, col);
    CGContext::fill_ellipse_in_rect(c(ctx), rect);
    CGContext::set_shadow_with_color(c(ctx), CGSize::new(0.0, 0.0), 0.0, None);
    CGContext::set_line_width(c(ctx), 1.5);
    set_stroke(ctx, Rgba(1.0, 1.0, 1.0, 0.95));
    CGContext::stroke_ellipse_in_rect(c(ctx), rect);
    let fg = if col.luminance() > 0.6 { Rgba(0.1, 0.1, 0.1, 1.0) } else { Rgba(1.0, 1.0, 1.0, 1.0) };
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(r * 1.05, unsafe { NSFontWeightBold });
    let s = attributed(&n.to_string(), &font, fg);
    let sz = s.size();
    with_ns_context(ctx, || {
        s.drawAtPoint(objc2_foundation::NSPoint::new(center.x - sz.width / 2.0, center.y - sz.height / 2.0));
    });
}

fn draw_pixelate(ctx: &CGContext, r: Rect, block: f64, src: &Source) {
    if r.is_empty() {
        return;
    }
    let px = view_to_image_px(r, src.view_h, src.scale);
    let Some(crop) = CGImage::with_image_in_rect(Some(src.image), cg_rect(px)) else { return };
    let (cw, ch) = (((r.w / block).ceil() as usize).max(1), ((r.h / block).ceil() as usize).max(1));
    let Some(small) = scaled_copy(&crop, cw, ch) else { return };
    CGContext::set_interpolation_quality(c(ctx), CGInterpolationQuality::None);
    CGContext::draw_image(c(ctx), cg_rect(r), Some(&small));
}

/// Draw `image` into a new `w`×`h` RGBA bitmap (averaging pixels when shrinking).
pub fn scaled_copy(image: &CGImage, w: usize, h: usize) -> Option<CFRetained<CGImage>> {
    let space = CGColorSpace::new_device_rgb()?;
    let ctx = unsafe {
        CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, 0, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::Medium);
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(image));
    CGBitmapContextCreateImage(Some(&ctx))
}

/// Size a text string would occupy (for the inline editor and bounds).
pub fn text_size(text: &str, size: f64) -> (f64, f64) {
    let font = text_font(size);
    let s = attributed(if text.is_empty() { " " } else { text }, &font, Rgba(0.0, 0.0, 0.0, 1.0));
    let sz = s.size();
    (sz.width, sz.height)
}
