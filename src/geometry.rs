//! Plain geometry, independent of AppKit so it can be unit-tested.
//!
//! Coordinate spaces:
//! - **CG global**: origin at the primary display's top-left, y grows down (window lists, SCK).
//! - **AppKit global**: origin at the primary display's bottom-left, y grows up (NSScreen, NSWindow).
//! - **View local**: AppKit space relative to one overlay window (one per display).
//! - **Image pixels**: origin top-left of a display capture, scaled by the display's backing scale.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub const fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

impl Point {
    pub fn dist(self, o: Point) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// Rect spanned by two corner points, in any order.
    pub fn from_points(a: Point, b: Point) -> Rect {
        Rect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid(&self) -> Point {
        pt(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn is_empty(&self) -> bool {
        self.w < 1.0 || self.h < 1.0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.max_x() && p.y >= self.y && p.y <= self.max_y()
    }

    pub fn inset(&self, d: f64) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }

    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let mx = self.max_x().min(o.max_x());
        let my = self.max_y().min(o.max_y());
        (mx > x && my > y).then(|| Rect::new(x, y, mx - x, my - y))
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Move (not resize) so the rect lies within `bounds` where possible.
    pub fn clamp_inside(&self, bounds: &Rect) -> Rect {
        let w = self.w.min(bounds.w);
        let h = self.h.min(bounds.h);
        let x = self.x.clamp(bounds.x, bounds.max_x() - w);
        let y = self.y.clamp(bounds.y, bounds.max_y() - h);
        Rect::new(x, y, w, h)
    }

    /// Snap edges to the pixel grid of a display with the given scale.
    pub fn snap(&self, scale: f64) -> Rect {
        let f = |v: f64| (v * scale).round() / scale;
        let x = f(self.x);
        let y = f(self.y);
        Rect::new(x, y, f(self.max_x()) - x, f(self.max_y()) - y)
    }

    /// Size in physical pixels.
    pub fn pixel_size(&self, scale: f64) -> (u32, u32) {
        ((self.w * scale).round().max(0.0) as u32, (self.h * scale).round().max(0.0) as u32)
    }
}

/// Convert a CG-global rect (top-left origin) to AppKit-global (bottom-left origin).
pub fn cg_to_appkit(r: Rect, primary_height: f64) -> Rect {
    Rect::new(r.x, primary_height - r.y - r.h, r.w, r.h)
}

/// Convert a view-local rect on a display of height `view_h` (points) to an image
/// pixel rect (top-left origin) at `scale`.
pub fn view_to_image_px(r: Rect, view_h: f64, scale: f64) -> Rect {
    Rect::new(r.x * scale, (view_h - r.y - r.h) * scale, r.w * scale, r.h * scale)
}

/// Snap the end point of a line so its angle is a multiple of 45°.
pub fn constrain_45(start: Point, end: Point) -> Point {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return end;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    pt(start.x + len * angle.cos(), start.y + len * angle.sin())
}

/// Make the rect from `anchor` to `p` square (for Shift-drag of rectangles/ellipses).
pub fn constrain_square(anchor: Point, p: Point) -> Point {
    let dx = p.x - anchor.x;
    let dy = p.y - anchor.y;
    let side = dx.abs().max(dy.abs());
    pt(anchor.x + side * dx.signum_or_one(), anchor.y + side * dy.signum_or_one())
}

trait SignumOrOne {
    fn signum_or_one(self) -> f64;
}
impl SignumOrOne for f64 {
    fn signum_or_one(self) -> f64 {
        if self < 0.0 { -1.0 } else { 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_points_normalises() {
        assert_eq!(Rect::from_points(pt(10.0, 20.0), pt(4.0, 2.0)), Rect::new(4.0, 2.0, 6.0, 18.0));
    }

    #[test]
    fn cg_appkit_conversion() {
        // A 100pt-tall window at the very top of a 900pt primary display.
        let r = cg_to_appkit(Rect::new(0.0, 0.0, 200.0, 100.0), 900.0);
        assert_eq!(r, Rect::new(0.0, 800.0, 200.0, 100.0));
    }

    #[test]
    fn view_to_pixels_flips_and_scales() {
        let r = view_to_image_px(Rect::new(10.0, 0.0, 20.0, 30.0), 100.0, 2.0);
        assert_eq!(r, Rect::new(20.0, 140.0, 40.0, 60.0));
    }

    #[test]
    fn clamp_keeps_rect_on_screen() {
        let b = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(Rect::new(90.0, -5.0, 20.0, 20.0).clamp_inside(&b), Rect::new(80.0, 0.0, 20.0, 20.0));
    }

    #[test]
    fn constrain_45_snaps_angles() {
        let e = constrain_45(pt(0.0, 0.0), pt(10.0, 1.0));
        assert!((e.y).abs() < 1e-9 && (e.x - 101f64.sqrt()).abs() < 1e-9);
        let d = constrain_45(pt(0.0, 0.0), pt(10.0, 9.0));
        assert!((d.x - d.y).abs() < 1e-9);
    }

    #[test]
    fn constrain_square_keeps_direction() {
        assert_eq!(constrain_square(pt(0.0, 0.0), pt(-10.0, 4.0)), pt(-10.0, 10.0));
    }

    #[test]
    fn snap_to_pixels() {
        let r = Rect::new(0.3, 0.26, 10.1, 10.0).snap(2.0);
        assert_eq!(r, Rect::new(0.5, 0.5, 10.0, 10.0));
    }
}
