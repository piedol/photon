//! On-screen recording indicators (never captured, since Photon's own windows
//! are excluded from the stream): the region outline and the countdown.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSFont, NSPanel, NSTextAlignment, NSTextField, NSView, NSFontWeightSemibold};
use objc2_core_graphics::CGMutablePath;
use objc2_foundation::{NSArray, NSNumber};
use objc2_quartz_core::{CALayer, CAShapeLayer, CATransaction};

use crate::capture::cg_rect;
use crate::geometry::Rect;
use crate::overlay::chrome::rgba;
use crate::{hud, ui};

pub struct Outline {
    panel: Retained<NSPanel>,
    border: Retained<CAShapeLayer>,
}

const PAD: f64 = 4.0;

impl Outline {
    pub fn show(mtm: MainThreadMarker, region: Rect) -> Outline {
        let frame = Rect::new(region.x - PAD, region.y - PAD, region.w + PAD * 2.0, region.h + PAD * 2.0);
        let panel = hud::panel(mtm, frame, objc2_app_kit::NSStatusWindowLevel);
        panel.setIgnoresMouseEvents(true);
        panel.setHasShadow(false);
        let view = NSView::initWithFrame(<NSView as objc2::MainThreadOnly>::alloc(mtm), cg_rect(Rect::new(0.0, 0.0, frame.w, frame.h)));
        let root = CALayer::new();
        view.setLayer(Some(&root));
        view.setWantsLayer(true);
        let border = CAShapeLayer::new();
        let path = CGMutablePath::new();
        unsafe { CGMutablePath::add_rect(Some(&path), std::ptr::null(), cg_rect(Rect::new(PAD - 1.5, PAD - 1.5, region.w + 3.0, region.h + 3.0))) };
        border.setPath(Some(&path));
        border.setFillColor(None);
        border.setLineWidth(1.5);
        root.addSublayer(&border);
        panel.setContentView(Some(&view));
        panel.orderFrontRegardless();
        let o = Outline { panel, border };
        o.set_recording(false);
        o
    }

    /// Red dashes while recording; grey while counting down or paused.
    pub fn set_recording(&self, on: bool) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let c: objc2_core_foundation::CFRetained<objc2_core_graphics::CGColor> = if on {
            NSColor::systemRedColor().colorWithAlphaComponent(0.95).CGColor().into()
        } else {
            rgba(1.0, 1.0, 1.0, 0.6)
        };
        self.border.setStrokeColor(Some(&c));
        let dash = NSArray::from_retained_slice(&[NSNumber::new_f64(6.0), NSNumber::new_f64(4.0)]);
        self.border.setLineDashPattern(Some(&dash));
        CATransaction::commit();
    }
}

impl Drop for Outline {
    fn drop(&mut self) {
        self.panel.orderOut(None);
        self.panel.close();
    }
}

pub struct Countdown {
    panel: Retained<NSPanel>,
    label: Retained<NSTextField>,
}

impl Countdown {
    pub fn show(mtm: MainThreadMarker, region: Rect) -> Countdown {
        let size = 96.0;
        let c = region.mid();
        let frame = Rect::new(c.x - size / 2.0, c.y - size / 2.0, size, size);
        let panel = hud::panel(mtm, frame, objc2_app_kit::NSStatusWindowLevel + 1);
        panel.setIgnoresMouseEvents(true);
        let surface = ui::hud_surface(mtm, cg_rect(Rect::new(0.0, 0.0, size, size)), size / 2.0, false);
        let label = NSTextField::labelWithString(&crate::util::ns("3"), mtm);
        label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(44.0, unsafe { NSFontWeightSemibold })));
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setAlignment(NSTextAlignment::Center);
        label.setFrame(cg_rect(Rect::new(0.0, (size - 54.0) / 2.0, size, 54.0)));
        surface.addSubview(&label);
        panel.setContentView(Some(&surface));
        panel.orderFrontRegardless();
        Countdown { panel, label }
    }

    pub fn set(&self, n: u32) {
        self.label.setStringValue(&crate::util::ns(&n.to_string()));
        if n == 0 {
            self.panel.orderOut(None);
        }
    }
}

impl Drop for Countdown {
    fn drop(&mut self) {
        self.panel.orderOut(None);
        self.panel.close();
    }
}
