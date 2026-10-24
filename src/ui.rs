//! Shared AppKit plumbing: the controller object every control targets, and
//! small constructors for symbols, buttons and HUD surfaces.

use std::cell::OnceCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2::runtime::Sel;
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSColor, NSFont, NSImage,
    NSImageSymbolConfiguration, NSMenu, NSMenuDelegate, NSTextField, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSAppearance,
    NSAppearanceNameDarkAqua, NSFontWeightMedium, NSAppearanceCustomization, NSColorPanel,
    NSControlTextEditingDelegate, NSTextFieldDelegate, NSTextView, NSColorSpace,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSRect, NSURL};

use crate::commands::Command;
use crate::util::ns;

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonController"]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            crate::app::launched(self.mtm());
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            for url in urls.iter() {
                if let Some(host) = url.host() {
                    crate::app::open_url(&host.to_string());
                }
            }
        }
    }

    unsafe impl NSMenuDelegate for Controller {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: &NSMenu) {
            crate::app::menu_will_open();
        }
    }

    unsafe impl NSControlTextEditingDelegate for Controller {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _n: &NSNotification) {
            crate::session::text_changed();
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &AnyObject, _tv: &NSTextView, selector: Sel) -> bool {
            if selector == sel!(insertNewline:) {
                crate::session::text_finished(true);
                true
            } else if selector == sel!(cancelOperation:) {
                crate::session::text_finished(false);
                true
            } else {
                false
            }
        }
    }

    unsafe impl NSTextFieldDelegate for Controller {}

    impl Controller {
        #[unsafe(method(colorChanged:))]
        fn color_changed(&self, sender: Option<&AnyObject>) {
            let Some(panel) = sender.and_then(|s| s.downcast_ref::<NSColorPanel>()) else { return };
            let c = panel.color();
            if let Some(c) = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) {
                crate::session::custom_color_changed(crate::annotate::model::Rgba(
                    c.redComponent(), c.greenComponent(), c.blueComponent(), 1.0,
                ));
            }
        }

        #[unsafe(method(command:))]
        fn command(&self, sender: Option<&AnyObject>) {
            let tag: isize = match sender {
                Some(s) => unsafe { msg_send![s, tag] },
                None => 0,
            };
            if let Some(cmd) = Command::from_tag(tag) {
                crate::commands::run(cmd);
            }
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: Option<&AnyObject>) {
            crate::app::tick();
        }
    }
);

thread_local! {
    static CONTROLLER: OnceCell<Retained<Controller>> = const { OnceCell::new() };
}

/// The single controller instance (target of every button and menu item).
pub fn controller(mtm: MainThreadMarker) -> Retained<Controller> {
    CONTROLLER.with(|c| {
        c.get_or_init(|| {
            let this = Controller::alloc(mtm).set_ivars(());
            unsafe { msg_send![super(this), init] }
        })
        .clone()
    })
}

pub fn app(mtm: MainThreadMarker) -> Retained<NSApplication> {
    NSApplication::sharedApplication(mtm)
}

pub fn set_app_delegate(mtm: MainThreadMarker) {
    let c = controller(mtm);
    app(mtm).setDelegate(Some(ProtocolObject::from_ref(&*c)));
}

pub fn text_delegate(mtm: MainThreadMarker) -> Retained<ProtocolObject<dyn NSTextFieldDelegate>> {
    ProtocolObject::from_retained(controller(mtm))
}

pub fn menu_delegate(mtm: MainThreadMarker) -> Retained<ProtocolObject<dyn NSMenuDelegate>> {
    ProtocolObject::from_retained(controller(mtm))
}

/// An SF Symbol, optionally sized.
pub fn symbol(name: &str, points: f64) -> Option<Retained<NSImage>> {
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)?;
    if points > 0.0 {
        let cfg = NSImageSymbolConfiguration::configurationWithPointSize_weight(points, unsafe { NSFontWeightMedium });
        return img.imageWithSymbolConfiguration(&cfg).or(Some(img));
    }
    Some(img)
}

/// Borderless icon button that sends `command:` with `cmd` as its tag.
pub fn icon_button(mtm: MainThreadMarker, symbol_name: &str, tooltip: &str, cmd: Command) -> Retained<NSButton> {
    let image = symbol(symbol_name, 15.0).unwrap_or_else(|| symbol("questionmark", 15.0).unwrap());
    let target = controller(mtm);
    let b = unsafe {
        NSButton::buttonWithImage_target_action(&image, Some(&target), Some(sel!(command:)), mtm)
    };
    b.setBordered(false);
    b.setTag(cmd.tag());
    b.setToolTip(Some(&ns(tooltip)));
    b.setContentTintColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.92)));
    b
}

/// Text push button that sends `command:`.
pub fn text_button(mtm: MainThreadMarker, title: &str, cmd: Command) -> Retained<NSButton> {
    let target = controller(mtm);
    let b = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(&target), Some(sel!(command:)), mtm) };
    b.setTag(cmd.tag());
    b
}

pub fn label(mtm: MainThreadMarker, text: &str, size: f64) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&ns(text), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    l
}

/// A dark HUD-material panel with rounded corners. `within` blurs this window's
/// own content (overlay toolbars); otherwise it blurs what is behind the window.
pub fn hud_surface(mtm: MainThreadMarker, frame: NSRect, radius: f64, within: bool) -> Retained<NSVisualEffectView> {
    let v = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    v.setMaterial(NSVisualEffectMaterial::HUDWindow);
    v.setState(NSVisualEffectState::Active);
    v.setBlendingMode(if within { NSVisualEffectBlendingMode::WithinWindow } else { NSVisualEffectBlendingMode::BehindWindow });
    v.setWantsLayer(true);
    if let Some(layer) = v.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
    dark(&v);
    v
}

/// Force dark appearance on a view tree.
pub fn dark(view: &NSView) {
    if let Some(a) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }) {
        view.setAppearance(Some(&a));
    }
}
