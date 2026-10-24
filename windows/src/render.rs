//! Direct2D drawing of markup shapes (the shared `model::Shape`). Coordinates are
//! physical pixels (y down); `scale` (DPI / 96) sizes strokes and text so marks
//! look the same as on the Mac. Used for the live overlay and for export.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::D2D1_QUADRATIC_BEZIER_SEGMENT;
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use crate::d2d::{self, rgba};
use crate::geometry::{Point, Rect};
use crate::model::{Rgba, Shape};

/// The frozen frame (BGRA, top-down) for the pixelate tool.
pub struct Source<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

fn col(c: Rgba, alpha: f64) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
    rgba(c.0 as f32, c.1 as f32, c.2 as f32, (c.3 * alpha) as f32)
}

pub fn draw_shapes(rt: &ID2D1RenderTarget, shapes: &[Shape], src: &Source) {
    for s in shapes {
        draw_shape(rt, s, src);
    }
}

fn smooth_path(pts: &[Point]) -> Option<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    let first = pts.first()?;
    unsafe {
        let geo = d2d::factory().CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        sink.BeginFigure(d2d::pt(first.x, first.y), D2D1_FIGURE_BEGIN_HOLLOW);
        if pts.len() < 3 {
            for p in &pts[1..] {
                sink.AddLine(d2d::pt(p.x, p.y));
            }
        } else {
            // Quadratic curves through midpoints give smooth freehand strokes.
            for w in pts.windows(2).skip(1) {
                let (a, b) = (w[0], w[1]);
                let seg = D2D1_QUADRATIC_BEZIER_SEGMENT { point1: d2d::pt(a.x, a.y), point2: d2d::pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) };
                sink.AddQuadraticBezier(&seg);
            }
            let last = pts[pts.len() - 1];
            sink.AddLine(d2d::pt(last.x, last.y));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close().ok()?;
        Some(geo)
    }
}

pub fn draw_shape(rt: &ID2D1RenderTarget, shape: &Shape, src: &Source) {
    let k = src.scale;
    let round = d2d::round_stroke();
    unsafe {
        match shape {
            Shape::Pen { pts, style } => {
                if let Some(g) = smooth_path(pts) {
                    rt.DrawGeometry(&g, &d2d::brush(rt, col(style.color, 1.0)), (style.width() * k) as f32, &round);
                }
            }
            Shape::Marker { pts, style } => {
                if let Some(g) = smooth_path(pts) {
                    rt.DrawGeometry(&g, &d2d::brush(rt, col(style.color, 0.38)), (style.marker_width() * k) as f32, &round);
                }
            }
            Shape::Line { a, b, style } => {
                rt.DrawLine(d2d::v2(a.x, a.y), d2d::v2(b.x, b.y), &d2d::brush(rt, col(style.color, 1.0)), (style.width() * k) as f32, &round);
            }
            Shape::Arrow { a, b, style } => draw_arrow(rt, *a, *b, style.width() * k, style.color),
            Shape::Rect { r, style } => {
                rt.DrawRectangle(&d2d::rect(r.x, r.y, r.w, r.h), &d2d::brush(rt, col(style.color, 1.0)), (style.width() * k) as f32, None);
            }
            Shape::Ellipse { r, style } => {
                let e = d2d::ellipse(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0);
                rt.DrawEllipse(&e, &d2d::brush(rt, col(style.color, 1.0)), (style.width() * k) as f32, None);
            }
            Shape::Text { origin, text, style } => {
                if !text.is_empty() {
                    let l = d2d::layout(text, style.font_size() * k, d2d::WEIGHT_SEMIBOLD);
                    rt.DrawTextLayout(d2d::v2(origin.x, origin.y), &l, &d2d::brush(rt, col(style.color, 1.0)), D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
            }
            Shape::Blur { r, style } => draw_pixelate(rt, *r, style.blur_block() * k, src),
            Shape::Step { center, n, style } => draw_step(rt, *center, *n, style.step_radius() * k, style.color, k),
        }
    }
}

fn draw_arrow(rt: &ID2D1RenderTarget, a: Point, b: Point, w: f64, c: Rgba) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = (w * 4.5 + 6.0).min(len * 0.6);
    let half = head * 0.42;
    let base = (b.x - ux * head, b.y - uy * head);
    let brush = d2d::brush(rt, col(c, 1.0));
    let round = d2d::round_stroke();
    unsafe {
        rt.DrawLine(d2d::v2(a.x, a.y), d2d::v2(base.0 + ux * head * 0.3, base.1 + uy * head * 0.3), &brush, w as f32, &round);
    }
    let tri = [
        (b.x, b.y),
        (base.0 - uy * half, base.1 + ux * half),
        (base.0 + ux * head * 0.12, base.1 + uy * head * 0.12),
        (base.0 + uy * half, base.1 - ux * half),
    ];
    if let Some(g) = d2d::polyline(&tri, true) {
        unsafe {
            rt.FillGeometry(&g, &brush, None);
            rt.DrawGeometry(&g, &brush, 1.0, &round);
        }
    }
}

fn draw_step(rt: &ID2D1RenderTarget, c: Point, n: u32, r: f64, color: Rgba, k: f64) {
    unsafe {
        let shadow = d2d::brush(rt, rgba(0.0, 0.0, 0.0, 0.25));
        rt.FillEllipse(&d2d::ellipse(c.x, c.y + 1.0 * k, r + 1.0 * k, r + 1.0 * k), &shadow);
        rt.FillEllipse(&d2d::ellipse(c.x, c.y, r, r), &d2d::brush(rt, col(color, 1.0)));
        rt.DrawEllipse(&d2d::ellipse(c.x, c.y, r, r), &d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.95)), (1.5 * k) as f32, None);
    }
    let fg = if color.luminance() > 0.6 { rgba(0.1, 0.1, 0.1, 1.0) } else { rgba(1.0, 1.0, 1.0, 1.0) };
    d2d::draw_text_centered(rt, &n.to_string(), d2d::rect(c.x - r, c.y - r, r * 2.0, r * 2.0), r * 1.1, d2d::WEIGHT_BOLD, fg);
}

/// Block-average the frozen pixels under `r`, then draw them scaled up with
/// nearest-neighbour sampling, so the original detail is gone.
fn draw_pixelate(rt: &ID2D1RenderTarget, r: Rect, block: f64, src: &Source) {
    let x0 = r.x.max(0.0).floor() as i64;
    let y0 = r.y.max(0.0).floor() as i64;
    let x1 = (r.max_x().min(src.width as f64)).ceil() as i64;
    let y1 = (r.max_y().min(src.height as f64)).ceil() as i64;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let block = block.max(2.0);
    let cw = (((x1 - x0) as f64) / block).ceil().max(1.0) as usize;
    let ch = (((y1 - y0) as f64) / block).ceil().max(1.0) as usize;
    let mut out = vec![0u8; cw * ch * 4];
    let stride = src.width as usize * 4;
    for by in 0..ch {
        for bx in 0..cw {
            let sx0 = x0 as usize + (bx as f64 * block) as usize;
            let sy0 = y0 as usize + (by as f64 * block) as usize;
            let sx1 = ((x0 as f64 + (bx + 1) as f64 * block) as usize).min(x1 as usize);
            let sy1 = ((y0 as f64 + (by + 1) as f64 * block) as usize).min(y1 as usize);
            let (mut b, mut g, mut rr, mut n) = (0u64, 0u64, 0u64, 0u64);
            // Sample every other pixel; plenty for an average, twice as fast.
            for y in (sy0..sy1.max(sy0 + 1)).step_by(2) {
                for x in (sx0..sx1.max(sx0 + 1)).step_by(2) {
                    let i = y * stride + x * 4;
                    if i + 3 < src.pixels.len() {
                        b += src.pixels[i] as u64;
                        g += src.pixels[i + 1] as u64;
                        rr += src.pixels[i + 2] as u64;
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            let o = (by * cw + bx) * 4;
            out[o] = (b / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (rr / n) as u8;
            out[o + 3] = 255;
        }
    }
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    unsafe {
        if let Ok(bmp) = rt.CreateBitmap(D2D_SIZE_U { width: cw as u32, height: ch as u32 }, Some(out.as_ptr() as _), (cw * 4) as u32, &props) {
            let dest = d2d::rect(x0 as f64, y0 as f64, cw as f64 * block, ch as f64 * block);
            rt.PushAxisAlignedClip(&d2d::rect(r.x, r.y, r.w, r.h), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
            rt.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None);
            rt.PopAxisAlignedClip();
        }
    }
}

/// Size of text as the Text tool will draw it.
pub fn text_size(text: &str, font_size: f64, k: f64) -> (f64, f64) {
    let l = d2d::layout(if text.is_empty() { " " } else { text }, font_size * k, d2d::WEIGHT_SEMIBOLD);
    d2d::measure(&l)
}
