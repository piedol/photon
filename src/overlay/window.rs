//! The overlay window classes: a borderless key-capable window per display,
//! an event view that routes input to the session, and a view that draws markup.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSGraphicsContext, NSScreenSaverWindowLevel, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect};

use crate::geometry::{pt, Point};
use crate::session;

define_class!(
    // SAFETY: NSWindow subclass with no extra requirements; no Drop impl.
    #[unsafe(super = NSWindow)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonOverlayWindow"]
    pub struct OverlayWindow;

    impl OverlayWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }
    }
);

pub struct ViewIvars {
    pub index: Cell<usize>,
}

define_class!(
    // SAFETY: NSView subclass; ivars are plain Cells; no Drop impl.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonOverlayView"]
    #[ivars = ViewIvars]
    pub struct OverlayView;

    impl OverlayView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            session::mouse(self.idx(), session::MouseKind::Down, self.point(e), e);
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            session::mouse(self.idx(), session::MouseKind::Drag, self.point(e), e);
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, e: &NSEvent) {
            session::mouse(self.idx(), session::MouseKind::Up, self.point(e), e);
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, e: &NSEvent) {
            session::mouse(self.idx(), session::MouseKind::Move, self.point(e), e);
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _e: &NSEvent) {
            session::right_click(self.idx());
        }
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            session::scroll(e);
        }
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, e: &NSEvent) {
            session::flags_changed(self.idx(), e);
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, e: &NSEvent) {
            if !session::key_down(e) {
                unsafe { msg_send![super(self), keyDown: e] }
            }
        }
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, e: &NSEvent) -> Bool {
            Bool::new(session::key_equivalent(e))
        }
        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self, e: &NSEvent) {
            session::cursor_update(self.idx(), self.point(e));
        }
    }
);

impl OverlayView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, index: usize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars { index: Cell::new(index) });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let opts = NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect
            | NSTrackingAreaOptions::CursorUpdate
            | NSTrackingAreaOptions::MouseEnteredAndExited;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                frame,
                opts,
                Some(&view as &AnyObject),
                None,
            )
        };
        view.addTrackingArea(&area);
        view
    }

    fn idx(&self) -> usize {
        self.ivars().index.get()
    }

    fn point(&self, e: &NSEvent) -> Point {
        let p = self.convertPoint_fromView(e.locationInWindow(), None);
        pt(p.x, p.y)
    }
}

define_class!(
    // SAFETY: NSView subclass; drawing only; no Drop impl.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonAnnotView"]
    #[ivars = ViewIvars]
    pub struct AnnotView;

    impl AnnotView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            let Some(ctx) = NSGraphicsContext::currentContext() else { return };
            let cg = ctx.CGContext();
            session::draw_annotations(self.ivars().index.get(), &cg, dirty);
        }
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _p: NSPoint) -> Option<&NSView> {
            None // clicks fall through to the overlay view
        }
    }
);

impl AnnotView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect, index: usize) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ViewIvars { index: Cell::new(index) });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

/// A borderless window covering `frame` (AppKit global), above the menu bar and Dock.
pub fn make_window(mtm: MainThreadMarker, frame: NSRect) -> Retained<OverlayWindow> {
    let this = OverlayWindow::alloc(mtm).set_ivars(());
    let w: Retained<OverlayWindow> = unsafe {
        msg_send![super(this), initWithContentRect: frame, styleMask: NSWindowStyleMask::Borderless, backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe { w.setReleasedWhenClosed(false) };
    w.setLevel(NSScreenSaverWindowLevel);
    w.setOpaque(true);
    w.setHasShadow(false);
    w.setBackgroundColor(Some(&NSColor::blackColor()));
    w.setAnimationBehavior(NSWindowAnimationBehavior::None);
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    w.setAcceptsMouseMovedEvents(true);
    w.setHidesOnDeactivate(false);
    w.setFrame_display(frame, false);
    w
}
