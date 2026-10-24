//! Selection chrome drawn with Core Animation layers (GPU-composited, so moving
//! the selection never repaints the frozen frame): dim, border, handles, hover.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSColor, NSView};
use objc2_core_foundation::{CFRetained, CGPoint, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_foundation::{NSArray, NSNumber};
use objc2_quartz_core::{kCAFillRuleEvenOdd, CALayer, CAShapeLayer, CATransaction};

use super::selection::Handle;
use crate::capture::cg_rect;
use crate::geometry::Rect;

pub const DIM: f64 = 0.45;
const HANDLE: f64 = 7.0;

pub struct Chrome {
    pub view: Retained<NSView>,
    dim: Retained<CAShapeLayer>,
    hover: Retained<CAShapeLayer>,
    border: Retained<CAShapeLayer>,
    handles: Retained<CAShapeLayer>,
}

fn shape_layer() -> Retained<CAShapeLayer> {
    let l = CAShapeLayer::new();
    l.setFillColor(None);
    l.setStrokeColor(None);
    l
}

pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> CFRetained<CGColor> {
    CGColor::new_srgb(r, g, b, a)
}

pub fn accent(alpha: f64) -> CFRetained<CGColor> {
    let c = NSColor::controlAccentColor().colorWithAlphaComponent(alpha);
    c.CGColor().into()
}

fn rect_path(rects: &[Rect]) -> CFRetained<CGMutablePath> {
    let p = CGMutablePath::new();
    for r in rects {
        unsafe { CGMutablePath::add_rect(Some(&p), std::ptr::null(), cg_rect(*r)) };
    }
    p
}

impl Chrome {
    pub fn new(mtm: MainThreadMarker, bounds: Rect, scale: f64) -> Chrome {
        let view = NSView::initWithFrame(NSView::alloc(mtm), cg_rect(bounds));
        let root = CALayer::new();
        view.setLayer(Some(&root));
        view.setWantsLayer(true);

        let dim = shape_layer();
        dim.setFillColor(Some(&rgba(0.0, 0.0, 0.0, DIM)));
        dim.setFillRule(unsafe { kCAFillRuleEvenOdd });

        let hover = shape_layer();
        hover.setFillColor(Some(&accent(0.10)));
        hover.setStrokeColor(Some(&accent(0.95)));
        hover.setLineWidth(2.0);

        let border = shape_layer();
        border.setStrokeColor(Some(&rgba(1.0, 1.0, 1.0, 0.95)));
        border.setLineWidth(1.0);
        shadow(&border, 0.55, 1.5);

        let handles = shape_layer();
        handles.setFillColor(Some(&rgba(1.0, 1.0, 1.0, 1.0)));
        handles.setStrokeColor(Some(&rgba(0.0, 0.0, 0.0, 0.25)));
        handles.setLineWidth(0.5);
        shadow(&handles, 0.45, 1.5);

        for l in [&dim, &hover, &border, &handles] {
            l.setContentsScale(scale);
            root.addSublayer(l);
        }
        let c = Chrome { view, dim, hover, border, handles };
        c.update(bounds, None, None, false, false);
        c
    }

    /// Redraw the chrome. `hover` is the window-snap preview when nothing is selected.
    pub fn update(&self, bounds: Rect, sel: Option<Rect>, hover: Option<Rect>, handles: bool, dashed: bool) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let hole = sel.or(hover);
        let mut dim = vec![bounds];
        dim.extend(hole);
        self.dim.setPath(Some(&rect_path(&dim)));

        match (sel, hover) {
            (None, Some(h)) => self.hover.setPath(Some(&rect_path(&[h.inset(1.0)]))),
            _ => self.hover.setPath(None),
        }

        match sel {
            Some(s) => {
                self.border.setPath(Some(&rect_path(&[s.inset(-0.5)])));
                let pattern = dashed.then(|| NSArray::from_retained_slice(&[NSNumber::new_f64(5.0), NSNumber::new_f64(4.0)]));
                self.border.setLineDashPattern(pattern.as_deref());
            }
            None => self.border.setPath(None),
        }

        match sel.filter(|_| handles) {
            Some(s) => {
                let squares: Vec<Rect> = Handle::ALL
                    .iter()
                    .map(|h| {
                        let c = h.center(&s);
                        Rect::new(c.x - HANDLE / 2.0, c.y - HANDLE / 2.0, HANDLE, HANDLE)
                    })
                    .collect();
                self.handles.setPath(Some(&rect_path(&squares)));
            }
            None => self.handles.setPath(None),
        }
        CATransaction::commit();
    }
}

fn shadow(l: &CALayer, opacity: f32, radius: f64) {
    l.setShadowColor(Some(&rgba(0.0, 0.0, 0.0, 1.0)));
    l.setShadowOpacity(opacity);
    l.setShadowRadius(radius);
    l.setShadowOffset(CGSize::new(0.0, 0.0));
    let _ = CGPoint::new(0.0, 0.0);
}
