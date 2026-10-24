//! App lifecycle, the menu-bar item, and command routing.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSCellImagePosition, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEventMask, NSEventModifierFlags, NSEventType, NSFont, NSFontWeightMedium,
    NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSTimer;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use objc2_foundation::NSString;

use crate::commands::Command;
use crate::keys::{self, Shortcut};
use crate::util::{self, ns};
use crate::overlay::toolbar::Mode;
use crate::{hotkey, record, session, settings, ui};

/// Crop corners around a dot: the app icon's motif, distinct from other menu-bar logos.
const MENU_SYMBOL: &str = "dot.viewfinder";

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
    crate::output::destination::clean_scratch();
    if !crate::permissions::screen_ok() {
        crate::permissions::ask_screen();
    }
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
        if let Some(img) = ui::symbol(MENU_SYMBOL, 0.0) {
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

/// `photon://capture`, `photon://record`, `photon://stop`, `photon://settings`.
pub fn open_url(host: &str) {
    let cmd = match host {
        "capture" | "screenshot" => Command::CaptureScreenshot,
        "record" => Command::RecordScreen,
        "stop" => Command::StopRecording,
        "settings" => Command::OpenSettings,
        "selftest" => return crate::selftest::run(),
        "selftest-record" => return crate::selftest::run_record(),
        "selftest-settings" => return crate::selftest::run_settings(),
        "selftest-window" => return crate::selftest::run_window(),
        "selftest-escape" => return crate::selftest::run_escape(),
        _ => return,
    };
    handle(cmd);
}

pub fn menu_will_open() {
    let on = login_item_enabled();
    SHELL.with(|sh| {
        if let Some(sh) = sh.borrow().as_ref() {
            sh.login_item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        }
    });
}

pub fn login_item_enabled() -> bool {
    let status = unsafe { SMAppService::mainAppService().status() };
    status == SMAppServiceStatus::Enabled
}

/// Register or unregister Photon as a login item.
pub fn set_login_item(on: bool) -> Result<(), String> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = if on { unsafe { service.registerAndReturnError() } } else { unsafe { service.unregisterAndReturnError() } };
    result.map_err(|e| e.localizedDescription().to_string())
}

pub fn toggle_login_item() {
    if let Err(e) = set_login_item(!login_item_enabled()) {
        crate::hud::message("Couldn't change Launch at Login", Some(&e));
    }
}

thread_local! {
    static TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
    static RECORD_MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
}

/// Switch the menu-bar item between its idle look and the recording clock.
pub fn set_recording_ui(on: bool) {
    let mtm = mtm();
    TIMER.with(|t| {
        if let Some(t) = t.borrow_mut().take() {
            t.invalidate();
        }
    });
    SHELL.with(|sh| {
        let sh = sh.borrow();
        let Some(sh) = sh.as_ref() else { return };
        let Some(button) = sh.item.button(mtm) else { return };
        if on {
            sh.item.setMenu(None);
            let target = ui::controller(mtm);
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(statusClicked:)));
            }
            button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
            button.setImagePosition(NSCellImagePosition::ImageLeading);
            if let Some(img) = ui::symbol("record.circle.fill", 0.0) {
                // Palette colours keep the dot red; template images would be tinted white.
                let red = objc2_foundation::NSArray::from_retained_slice(&[NSColor::systemRedColor()]);
                let cfg = objc2_app_kit::NSImageSymbolConfiguration::configurationWithPaletteColors(&red);
                let img = img.imageWithSymbolConfiguration(&cfg).unwrap_or(img);
                img.setTemplate(false);
                button.setImage(Some(&img));
            }
            button.setContentTintColor(Some(&NSColor::systemRedColor()));
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })));
            button.setToolTip(Some(&ns("Click to stop recording · right-click for options")));
        } else {
            unsafe {
                button.setTarget(None);
                button.setAction(None);
            }
            button.setTitle(&NSString::new());
            button.setImagePosition(NSCellImagePosition::ImageOnly);
            if let Some(img) = ui::symbol(MENU_SYMBOL, 0.0) {
                img.setTemplate(true);
                button.setImage(Some(&img));
            }
            button.setContentTintColor(None);
            button.setToolTip(Some(&ns("Photon")));
            sh.item.setMenu(Some(&sh.menu));
        }
    });
    if on {
        let target = ui::controller(mtm);
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.5, &target, sel!(tick:), None, true)
        };
        TIMER.with(|t| *t.borrow_mut() = Some(timer));
        refresh_recording_ui();
    }
}

/// Update the clock text ("● 02:14", or "Paused").
pub fn refresh_recording_ui() {
    let mtm = mtm();
    if !record::is_active() {
        return;
    }
    let secs = record::elapsed().as_secs();
    let text = if record::is_paused() {
        format!(" Paused {:02}:{:02}", secs / 60, secs % 60)
    } else if secs >= 3600 {
        format!(" {}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        format!(" {:02}:{:02}", secs / 60, secs % 60)
    };
    SHELL.with(|sh| {
        if let Some(button) = sh.borrow().as_ref().and_then(|sh| sh.item.button(mtm)) {
            button.setTitle(&ns(&text));
        }
    });
}

/// One tick of the recording clock.
pub fn tick() {
    refresh_recording_ui();
}

/// Status item clicked while recording: left stops, right shows options.
pub fn status_clicked() {
    let mtm = mtm();
    let right = ui::app(mtm)
        .currentEvent()
        .map(|e| e.r#type() == NSEventType::RightMouseUp || e.modifierFlags().contains(NSEventModifierFlags::Control))
        .unwrap_or(false);
    if !right {
        record::stop(false);
        return;
    }
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    menu.addItem(&menu_item(mtm, "Stop and Save", Command::StopRecording));
    menu.addItem(&menu_item(mtm, if record::is_paused() { "Resume" } else { "Pause" }, Command::PauseResume));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(mtm, "Discard Recording", Command::Setting(999)));
    SHELL.with(|sh| {
        if let Some(sh) = sh.borrow().as_ref()
            && let Some(button) = sh.item.button(mtm) {
                let p = objc2_foundation::NSPoint::new(0.0, button.bounds().size.height + 4.0);
                menu.popUpMenuPositioningItem_atLocation_inView(None, p, Some(&button));
            }
    });
    RECORD_MENU.with(|m| *m.borrow_mut() = Some(menu));
}

pub fn handle(cmd: Command) {
    let mtm = mtm();
    match cmd {
        // While recording, either shortcut ends the recording.
        Command::CaptureScreenshot | Command::RecordHotkey if record::is_active() => record::stop(false),
        Command::CaptureScreenshot => session::start(Mode::Screenshot),
        Command::RecordScreen => session::start(Mode::Record),
        Command::RecordHotkey => session::start(Mode::Record),
        Command::SkipCountdown => record::skip_countdown(),
        Command::Escape => {
            if session::is_open() {
                session::escape();
            } else if record::is_counting_down() {
                record::stop(true);
            }
        }
        Command::Quit => {
            if record::is_active() {
                record::stop(false);
                util::main_after(2.0, || ui::app(self::mtm()).terminate(None));
            } else {
                ui::app(mtm).terminate(None);
            }
        }
        Command::StopRecording => record::stop(false),
        Command::PauseResume => record::toggle_pause(),
        Command::Setting(999) => record::stop(true),
        Command::Setting(id) => crate::settings_window::changed(id),
        Command::ToggleLogin => toggle_login_item(),
        Command::OpenSettings => crate::settings_window::show(),
        Command::Reveal | Command::CopyFile | Command::OpenFile | Command::Trim | Command::ExportGif | Command::DismissHud => {
            record::post::command(cmd)
        }
        Command::OpenRecordings => {
            let dir = settings::get().rec_dir;
            let _ = std::fs::create_dir_all(&dir);
            let url = objc2_foundation::NSURL::fileURLWithPath(&ns(&dir.to_string_lossy()));
            objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
        }
        c if session::is_open() => session::command(c),
        _ => util::log(format!("unhandled {cmd:?}")),
    }
}
