//! Selection state machine for one overlay: create, move, resize, nudge. Pure logic.

use crate::geometry::{pt, Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Handle {
    pub const ALL: [Handle; 8] = [Handle::N, Handle::S, Handle::E, Handle::W, Handle::NE, Handle::NW, Handle::SE, Handle::SW];

    /// Centre of this handle on `r` (AppKit coordinates: N is the max-y edge).
    pub fn center(self, r: &Rect) -> Point {
        let (mx, my) = (r.mid().x, r.mid().y);
        match self {
            Handle::N => pt(mx, r.max_y()),
            Handle::S => pt(mx, r.y),
            Handle::E => pt(r.max_x(), my),
            Handle::W => pt(r.x, my),
            Handle::NE => pt(r.max_x(), r.max_y()),
            Handle::NW => pt(r.x, r.max_y()),
            Handle::SE => pt(r.max_x(), r.y),
            Handle::SW => pt(r.x, r.y),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Idle,
    /// Mouse down with no selection yet; becomes Creating once it moves.
    Pressed { at: Point },
    Creating { anchor: Point },
    Moving { grab: Point, orig: Rect },
    Resizing { handle: Handle, orig: Rect },
}

/// What a mouse-up produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Release {
    Nothing,
    /// A click with no drag (caller may snap to the hovered window or display).
    Click(Point),
    Changed,
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub rect: Option<Rect>,
    pub bounds: Rect,
    pub scale: f64,
    drag: Drag,
}

/// Hit slop around handles, in points.
pub const HANDLE_HIT: f64 = 7.0;
const MIN_SIZE: f64 = 4.0;
const CLICK_SLOP: f64 = 3.0;

impl Selection {
    pub fn new(bounds: Rect, scale: f64) -> Self {
        Selection { rect: None, bounds, scale, drag: Drag::Idle }
    }

    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag, Drag::Idle)
    }

    pub fn handle_at(&self, p: Point) -> Option<Handle> {
        let r = self.rect?;
        Handle::ALL.into_iter().find(|h| h.center(&r).dist(p) <= HANDLE_HIT)
    }

    pub fn set(&mut self, r: Rect) {
        self.rect = r.intersection(&self.bounds).map(|r| r.snap(self.scale));
    }

    /// Mouse down. `allow_new` is false once annotations exist, so a stray drag
    /// outside the selection can't throw work away. `allow_move` is false while a
    /// drawing tool is active (drags inside the selection draw instead).
    /// Returns false if the press isn't a selection gesture (the caller then draws).
    pub fn press(&mut self, p: Point, allow_new: bool, allow_move: bool) -> bool {
        if let Some(h) = self.handle_at(p) {
            self.drag = Drag::Resizing { handle: h, orig: self.rect.unwrap() };
            return true;
        }
        match self.rect {
            Some(r) if r.contains(p) => {
                if allow_move {
                    self.drag = Drag::Moving { grab: p, orig: r };
                    true
                } else {
                    false
                }
            }
            Some(_) if !allow_new => false,
            _ => {
                self.drag = Drag::Pressed { at: p };
                true
            }
        }
    }

    pub fn drag_to(&mut self, p: Point) {
        let b = self.bounds;
        let p = pt(p.x.clamp(b.x, b.max_x()), p.y.clamp(b.y, b.max_y()));
        match self.drag {
            Drag::Idle => {}
            Drag::Pressed { at } => {
                if at.dist(p) > CLICK_SLOP {
                    self.rect = None;
                    self.drag = Drag::Creating { anchor: at };
                    self.drag_to(p);
                }
            }
            Drag::Creating { anchor } => {
                self.rect = Some(Rect::from_points(anchor, p).snap(self.scale));
            }
            Drag::Moving { grab, orig } => {
                let r = orig.translate(p.x - grab.x, p.y - grab.y).clamp_inside(&b);
                self.rect = Some(r.snap(self.scale));
            }
            Drag::Resizing { handle, orig } => {
                let (mut x0, mut y0, mut x1, mut y1) = (orig.x, orig.y, orig.max_x(), orig.max_y());
                match handle {
                    Handle::N => y1 = p.y,
                    Handle::S => y0 = p.y,
                    Handle::E => x1 = p.x,
                    Handle::W => x0 = p.x,
                    Handle::NE => (x1, y1) = (p.x, p.y),
                    Handle::NW => (x0, y1) = (p.x, p.y),
                    Handle::SE => (x1, y0) = (p.x, p.y),
                    Handle::SW => (x0, y0) = (p.x, p.y),
                }
                self.rect = Some(Rect::from_points(pt(x0, y0), pt(x1, y1)).snap(self.scale));
            }
        }
    }

    pub fn release(&mut self, p: Point) -> Release {
        let drag = std::mem::replace(&mut self.drag, Drag::Idle);
        match drag {
            Drag::Idle => Release::Nothing,
            Drag::Pressed { .. } => Release::Click(p),
            Drag::Creating { .. } | Drag::Resizing { .. } => {
                if self.rect.is_some_and(|r| r.w < MIN_SIZE || r.h < MIN_SIZE) {
                    self.rect = None;
                }
                Release::Changed
            }
            Drag::Moving { .. } => Release::Changed,
        }
    }

    /// Arrow-key nudge. `dx`/`dy` are in physical pixels. With `resize`, the
    /// right/top edges move instead (selection grows or shrinks).
    pub fn nudge(&mut self, dx: f64, dy: f64, resize: bool) -> bool {
        let Some(r) = self.rect else { return false };
        let (dx, dy) = (dx / self.scale, dy / self.scale);
        let next = if resize {
            let w = (r.w + dx).max(MIN_SIZE);
            let h = (r.h + dy).max(MIN_SIZE);
            // Grow upward from the top edge in AppKit space means keeping y fixed.
            Rect::new(r.x, r.y, w, h).intersection(&self.bounds).unwrap_or(r)
        } else {
            r.translate(dx, dy).clamp_inside(&self.bounds)
        };
        self.rect = Some(next.snap(self.scale));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel() -> Selection {
        Selection::new(Rect::new(0.0, 0.0, 1000.0, 800.0), 2.0)
    }

    #[test]
    fn drag_creates_selection() {
        let mut s = sel();
        assert!(s.press(pt(100.0, 100.0), true, true));
        s.drag_to(pt(300.0, 50.0));
        assert_eq!(s.release(pt(300.0, 50.0)), Release::Changed);
        assert_eq!(s.rect, Some(Rect::new(100.0, 50.0, 200.0, 50.0)));
    }

    #[test]
    fn click_without_drag_is_a_click() {
        let mut s = sel();
        s.press(pt(10.0, 10.0), true, true);
        s.drag_to(pt(11.0, 11.0));
        assert_eq!(s.release(pt(11.0, 11.0)), Release::Click(pt(11.0, 11.0)));
        assert!(s.rect.is_none());
    }

    #[test]
    fn drag_is_clamped_to_display() {
        let mut s = sel();
        s.press(pt(900.0, 700.0), true, true);
        s.drag_to(pt(5000.0, 5000.0));
        s.release(pt(0.0, 0.0));
        assert_eq!(s.rect, Some(Rect::new(900.0, 700.0, 100.0, 100.0)));
    }

    #[test]
    fn move_and_resize() {
        let mut s = sel();
        s.set(Rect::new(100.0, 100.0, 100.0, 100.0));
        assert!(s.press(pt(150.0, 150.0), true, true));
        s.drag_to(pt(160.0, 140.0));
        s.release(pt(160.0, 140.0));
        assert_eq!(s.rect, Some(Rect::new(110.0, 90.0, 100.0, 100.0)));

        // NE handle at (210, 190)
        assert!(s.press(pt(211.0, 191.0), true, true));
        s.drag_to(pt(260.0, 240.0));
        s.release(pt(260.0, 240.0));
        assert_eq!(s.rect, Some(Rect::new(110.0, 90.0, 150.0, 150.0)));
    }

    #[test]
    fn resize_past_opposite_edge_flips() {
        let mut s = sel();
        s.set(Rect::new(100.0, 100.0, 100.0, 100.0));
        s.press(pt(200.0, 150.0), true, true); // E handle
        s.drag_to(pt(50.0, 150.0));
        s.release(pt(50.0, 150.0));
        assert_eq!(s.rect, Some(Rect::new(50.0, 100.0, 50.0, 100.0)));
    }

    #[test]
    fn tool_active_inside_is_not_a_selection_gesture() {
        let mut s = sel();
        s.set(Rect::new(100.0, 100.0, 100.0, 100.0));
        assert!(!s.press(pt(150.0, 150.0), true, false));
        // Handles still resize with a tool active.
        assert!(s.press(pt(100.0, 100.0), true, false));
    }

    #[test]
    fn annotations_protect_selection() {
        let mut s = sel();
        s.set(Rect::new(100.0, 100.0, 100.0, 100.0));
        assert!(!s.press(pt(500.0, 500.0), false, true));
        assert!(s.press(pt(500.0, 500.0), true, true));
    }

    #[test]
    fn nudge_moves_by_pixels() {
        let mut s = sel();
        s.set(Rect::new(100.0, 100.0, 100.0, 100.0));
        s.nudge(1.0, 0.0, false);
        assert_eq!(s.rect.unwrap().x, 100.5);
        s.nudge(10.0, 10.0, true);
        assert_eq!(s.rect.unwrap().w, 105.0);
        assert_eq!(s.rect.unwrap().h, 105.0);
        s.set(Rect::new(990.0, 0.0, 10.0, 10.0));
        s.nudge(20.0, 0.0, false);
        assert_eq!(s.rect.unwrap().x, 990.0);
    }

    #[test]
    fn tiny_drag_selection_discarded() {
        let mut s = sel();
        s.press(pt(100.0, 100.0), true, true);
        s.drag_to(pt(104.0, 101.0));
        s.release(pt(104.0, 101.0));
        assert!(s.rect.is_none());
    }
}
