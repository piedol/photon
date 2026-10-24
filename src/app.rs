//! App lifecycle, the menu-bar item, and command routing.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSEventModifierFlags, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSString;

use crate::commands::Command;
use crate::keys::{self, Shortcut};
use crate::util::{self, ns};
use crate::{hotkey, settings, ui};

struct Shell {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    shot_item: Retained<NSMenuItem>,
    record_item: Retained<NSMenuItem>,
    login_item: Retained<NSMenuItem>,
}

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("Photon must start on the main thread");
    let app = ui::app(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    ui::set_app_delegate(mtm);
    app.run();
}

pub fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("UI work must run on the main thread")
}

/// Called from `applicationDidFinishLaunching:`.
pub fn launched(mtm: MainThreadMarker) {
    build_status_item(mtm);
    apply_hotkeys();
    util::log("launched");
}

/// (Re)register hotkeys from settings and refresh the menu labels.
pub fn apply_hotkeys() -> Vec<String> {
    let s = settings::get();
    let errors = hotkey::install(s.shot_hotkey, s.record_hotkey);
    for e in &errors {
        util::log(e);
    }
    SHELL.with(|sh| {
        if let Some(sh) = sh.borrow().as_ref() {
            set_key_equivalent(&sh.shot_item, s.shot_hotkey);
            set_key_equivalent(&sh.record_item, s.record_hotkey);
        }
    });
    errors
}

fn set_key_equivalent(item: &NSMenuItem, sc: Shortcut) {
    if let Some(c) = sc.menu_char() {
        item.setKeyEquivalent(&ns(&c));
        let mut m = NSEventModifierFlags::empty();
        if sc.mods & keys::SHIFT != 0 { m |= NSEventModifierFlags::Shift; }
        if sc.mods & keys::CONTROL != 0 { m |= NSEventModifierFlags::Control; }
        if sc.mods & keys::OPTION != 0 { m |= NSEventModifierFlags::Option; }
        if sc.mods & keys::COMMAND != 0 { m |= NSEventModifierFlags::Command; }
        item.setKeyEquivalentModifierMask(m);
    } else {
        item.setKeyEquivalent(&NSString::new());
    }
}

fn menu_item(mtm: MainThreadMarker, title: &str, cmd: Command) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &ns(title),
            Some(sel!(command:)),
            &NSString::new(),
        )
    };
    let target = ui::controller(mtm);
    unsafe { item.setTarget(Some(&target)) };
    item.setTag(cmd.tag());
    item
}

fn build_status_item(mtm: MainThreadMarker) {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        if let Some(img) = ui::symbol("camera.aperture", 0.0) {
            img.setTemplate(true);
            button.setImage(Some(&img));
        }
        button.setToolTip(Some(&ns("Photon")));
    }

    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let shot_item = menu_item(mtm, "Capture Screenshot", Command::CaptureScreenshot);
    let record_item = menu_item(mtm, "Record Screen", Command::RecordScreen);
    let login_item = menu_item(mtm, "Launch at Login", Command::ToggleLogin);
    menu.addItem(&shot_item);
    menu.addItem(&record_item);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(mtm, "Open Recordings Folder", Command::OpenRecordings));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let settings_item = menu_item(mtm, "Settings…", Command::OpenSettings);
    settings_item.setKeyEquivalent(&ns(","));
    menu.addItem(&settings_item);
    menu.addItem(&login_item);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let quit = menu_item(mtm, "Quit Photon", Command::Quit);
    quit.setKeyEquivalent(&ns("q"));
    menu.addItem(&quit);
    menu.setDelegate(Some(&ui::menu_delegate(mtm)));
    item.setMenu(Some(&menu));

    SHELL.with(|sh| {
        *sh.borrow_mut() = Some(Shell { item, menu, shot_item, record_item, login_item })
    });
}

pub fn menu_will_open() {}

/// One-second timer tick (recording clock).
pub fn tick() {}

pub fn handle(cmd: Command) {
    let mtm = mtm();
    match cmd {
        Command::Quit => ui::app(mtm).terminate(None),
        _ => util::log(format!("unhandled {cmd:?}")),
    }
}
