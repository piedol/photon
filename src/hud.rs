//! Small transient HUD windows ("Copied", errors) near the pointer.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSBackingStoreType, NSColor, NSEvent, NSFont, NSImageView, NSPanel,
    NSScreen, NSTextField, NSWindowCollectionBehavior, NSWindowStyleMask, NSFontWeightSemibold,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::capture::cg_rect;
use crate::geometry::{pt, Rect};
use crate::ui;
use crate::util::{self, ns};

thread_local! {
    static WINDOWS: RefCell<Vec<(u64, Retained<NSPanel>)>> = const { RefCell::new(Vec::new()) };
    static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// A borderless, non-activating floating panel.
pub fn panel(mtm: MainThreadMarker, frame: Rect, level: isize) -> Retained<NSPanel> {
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        cg_rect(frame),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { p.setReleasedWhenClosed(false) };
    p.setLevel(level);
    p.setOpaque(false);
    p.setBackgroundColor(Some(&NSColor::clearColor()));
    p.setHasShadow(true);
    p.setHidesOnDeactivate(false);
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    p
}

/// Screen frame (AppKit global) containing the pointer.
pub fn pointer_screen(mtm: MainThreadMarker) -> Rect {
    let m = NSEvent::mouseLocation();
    for s in NSScreen::screens(mtm).iter() {
        let f = s.visibleFrame();
        let r = Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height);
        let full = s.frame();
        if Rect::new(full.origin.x, full.origin.y, full.size.width, full.size.height).contains(pt(m.x, m.y)) {
            return r;
        }
    }
    Rect::new(0.0, 0.0, 1440.0, 900.0)
}

fn register(p: Retained<NSPanel>) -> u64 {
    let id = NEXT.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    WINDOWS.with(|w| w.borrow_mut().push((id, p)));
    id
}

/// Fade out and release a HUD window after `secs`.
pub fn dismiss_after(id: u64, secs: f64) {
    util::main_after(secs, move || fade_out(id));
}

pub fn fade_out(id: u64) {
    let Some(p) = WINDOWS.with(|w| w.borrow().iter().find(|(i, _)| *i == id).map(|(_, p)| p.clone())) else { return };
    p.animator().setAlphaValue(0.0);
    util::main_after(0.2, move || {
        let p = WINDOWS.with(|w| {
            let mut w = w.borrow_mut();
            w.iter().position(|(i, _)| *i == id).map(|i| w.remove(i).1)
        });
        if let Some(p) = p {
            p.orderOut(None);
            p.close();
        }
    });
}

/// A quiet confirmation near the pointer, e.g. "Copied".
pub fn toast(title: &str, detail: Option<&str>) {
    show(title, detail, if detail.is_some() { 2.2 } else { 0.9 }, "checkmark.circle.fill");
}

/// An error or notice that stays a little longer.
pub fn message(title: &str, detail: Option<&str>) {
    show(title, detail, 4.0, "exclamationmark.triangle.fill");
}

fn show(title: &str, detail: Option<&str>, secs: f64, symbol: &str) {
    let mtm = MainThreadMarker::new().unwrap();
    let title_font = NSFont::systemFontOfSize_weight(13.0, unsafe { NSFontWeightSemibold });
    let t = NSTextField::labelWithString(&ns(title), mtm);
    t.setFont(Some(&title_font));
    t.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.95)));
    let d = detail.map(|d| {
        let l = NSTextField::labelWithString(&ns(d), mtm);
        l.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        l.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.6)));
        l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
        l
    });
    let tw = t.fittingSize().width;
    let dw = d.as_ref().map(|d| d.fittingSize().width.min(380.0)).unwrap_or(0.0);
    let w = 16.0 + 18.0 + 8.0 + tw.max(dw) + 16.0;
    let h = if d.is_some() { 50.0 } else { 34.0 };

    let screen = pointer_screen(mtm);
    let m = NSEvent::mouseLocation();
    let frame = Rect::new(m.x + 14.0, m.y - h - 14.0, w, h).clamp_inside(&screen.inset(8.0));
    let p = panel(mtm, frame, objc2_app_kit::NSScreenSaverWindowLevel + 2);
    p.setIgnoresMouseEvents(true);
    let surface = ui::hud_surface(mtm, NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h)), 10.0, false);
    if let Some(img) = ui::symbol(symbol, 15.0) {
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        let tint = if symbol.starts_with("check") { NSColor::systemGreenColor() } else { NSColor::systemYellowColor() };
        iv.setContentTintColor(Some(&tint));
        iv.setFrame(cg_rect(Rect::new(14.0, h / 2.0 - 10.0, 20.0, 20.0)));
        surface.addSubview(&iv);
    }
    match &d {
        Some(d) => {
            t.setFrame(cg_rect(Rect::new(42.0, 25.0, w - 52.0, 17.0)));
            d.setFrame(cg_rect(Rect::new(42.0, 8.0, w - 52.0, 15.0)));
            surface.addSubview(d);
        }
        None => t.setFrame(cg_rect(Rect::new(42.0, 8.0, w - 52.0, 17.0))),
    }
    surface.addSubview(&t);
    p.setContentView(Some(&surface));
    p.orderFrontRegardless();
    let id = register(p);
    dismiss_after(id, secs);
}

pub fn keep(p: Retained<NSPanel>) -> u64 {
    register(p)
}
