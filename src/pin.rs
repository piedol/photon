//! Pin to screen: a capture floating above other windows as a reference.
//! Drag to move, scroll to change opacity, double-click or Esc to close,
//! right-click for Copy / Save / Close.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSEvent, NSFloatingWindowLevel, NSMenu, NSMenuItem, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_foundation::{NSRect, NSString};

use crate::capture::cg_rect;
use crate::geometry::Rect;
use crate::i18n::tr;
use crate::keys::vk;
use crate::overlay::window::OverlayWindow;
use crate::session::Finished;
use crate::util::ns;
use crate::{export, hud};

pub struct PinIvars {
    id: Cell<u64>,
}

define_class!(
    // SAFETY: NSView subclass; no Drop impl.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonPinView"]
    #[ivars = PinIvars]
    struct PinView;

    impl PinView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool { true }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            if e.clickCount() >= 2 {
                close(self.ivars().id.get());
            } else if let Some(w) = self.window() {
                w.performWindowDragWithEvent(e);
            }
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, e: &NSEvent) {
            let mtm = self.mtm();
            let menu = NSMenu::new(mtm);
            for (title, action) in [(tr("Copy"), sel!(copyPin:)), (tr("Save…"), sel!(savePin:)), (tr("Close"), sel!(closePin:))] {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(&title), Some(action), &NSString::new())
                };
                unsafe { item.setTarget(Some(self)) };
                menu.addItem(&item);
            }
            NSMenu::popUpContextMenu_withEvent_forView(&menu, e, self);
        }
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            if let Some(w) = self.window() {
                let a = (w.alphaValue() + e.scrollingDeltaY() * 0.01).clamp(0.25, 1.0);
                w.setAlphaValue(a);
            }
        }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, e: &NSEvent) {
            match e.keyCode() {
                vk::ESCAPE => close(self.ivars().id.get()),
                vk::C if e.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Command) => copy(self.ivars().id.get()),
                _ => unsafe { msg_send![super(self), keyDown: e] },
            }
        }
        #[unsafe(method(copyPin:))]
        fn copy_pin(&self, _s: Option<&AnyObject>) { copy(self.ivars().id.get()); }
        #[unsafe(method(savePin:))]
        fn save_pin(&self, _s: Option<&AnyObject>) { save(self.ivars().id.get()); }
        #[unsafe(method(closePin:))]
        fn close_pin(&self, _s: Option<&AnyObject>) { close(self.ivars().id.get()); }
    }
);

struct Pin {
    id: u64,
    window: Retained<OverlayWindow>,
    finished: Finished,
}

thread_local! {
    static PINS: RefCell<Vec<Pin>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

pub fn show(f: Finished) {
    let mtm = MainThreadMarker::new().unwrap();
    let id = NEXT.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let frame = f.global_rect;
    let this = OverlayWindow::alloc(mtm).set_ivars(());
    let w: Retained<OverlayWindow> = unsafe {
        msg_send![super(this), initWithContentRect: cg_rect(frame), styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel, backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe { w.setReleasedWhenClosed(false) };
    w.setLevel(NSFloatingWindowLevel);
    w.setOpaque(false);
    w.setBackgroundColor(Some(&NSColor::clearColor()));
    w.setHasShadow(true);
    w.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);

    let view = PinView::alloc(mtm).set_ivars(PinIvars { id: Cell::new(id) });
    let view: Retained<PinView> = unsafe { msg_send![super(view), initWithFrame: cg_rect(Rect::new(0.0, 0.0, frame.w, frame.h))] };
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        let obj: &AnyObject = unsafe { &*(CFRetained::as_ptr(&f.image).as_ptr() as *const AnyObject) };
        unsafe { layer.setContents(Some(obj)) };
        layer.setCornerRadius(4.0);
        layer.setMasksToBounds(true);
        layer.setBorderWidth(0.5);
        layer.setBorderColor(Some(&crate::overlay::chrome::rgba(1.0, 1.0, 1.0, 0.25)));
    }
    view.setToolTip(Some(&ns(&tr("Drag to move · scroll for opacity · double-click to close"))));
    w.setContentView(Some(&view));
    w.makeKeyAndOrderFront(None);
    w.makeFirstResponder(Some(&view));
    PINS.with(|p| p.borrow_mut().push(Pin { id, window: w, finished: f }));
}

fn close(id: u64) {
    let pin = PINS.with(|p| {
        let mut p = p.borrow_mut();
        p.iter().position(|x| x.id == id).map(|i| p.remove(i))
    });
    if let Some(pin) = pin {
        pin.window.orderOut(None);
        pin.window.close();
    }
}

fn copy(id: u64) {
    PINS.with(|p| {
        if let Some(pin) = p.borrow().iter().find(|x| x.id == id) {
            let f = &pin.finished;
            export::copy_image_to_pasteboard(&f.image, (f.global_rect.w, f.global_rect.h));
            hud::toast(&tr("Copied"), None);
        }
    });
}

fn save(id: u64) {
    let f = PINS.with(|p| {
        p.borrow().iter().find(|x| x.id == id).map(|pin| Finished {
            image: pin.finished.image.clone(),
            px: pin.finished.px,
            display_name: pin.finished.display_name.clone(),
            front_app: pin.finished.front_app.clone(),
            global_rect: pin.finished.global_rect,
        })
    });
    if let Some(f) = f {
        export::save_with_panel(f);
    }
}

#[allow(unused)]
fn _rect(r: NSRect) -> Rect {
    crate::capture::rect(r)
}
