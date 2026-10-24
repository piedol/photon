//! Global shortcuts via RegisterHotKey on the message window. The process that
//! receives a hotkey may take the foreground, so the overlay gets the keyboard.

use std::cell::Cell;

use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey};

use crate::commands::Command;
use crate::keys::{vk, Shortcut};
use crate::util;

pub const ID_SHOT: i32 = 1;
pub const ID_RECORD: i32 = 2;
pub const ID_ESCAPE: i32 = 3;

thread_local! {
    static ACTIVE: Cell<(Option<Shortcut>, Option<Shortcut>)> = const { Cell::new((None, None)) };
    static ESC: Cell<bool> = const { Cell::new(false) };
}

fn register(id: i32, sc: Shortcut) -> bool {
    unsafe { RegisterHotKey(Some(util::message_window()), id, HOT_KEY_MODIFIERS(sc.mods) | MOD_NOREPEAT, sc.key as u32).is_ok() }
}

fn unregister(id: i32) {
    unsafe {
        let _ = UnregisterHotKey(Some(util::message_window()), id);
    }
}

/// (Re)register both shortcuts. Returns a message per failure.
pub fn install(shot: Shortcut, record: Shortcut) -> Vec<String> {
    unregister(ID_SHOT);
    unregister(ID_RECORD);
    let mut errors = Vec::new();
    let a = register(ID_SHOT, shot);
    if !a {
        errors.push(format!("{} is already used by another app (screenshot).", shot.label()));
    }
    let b = register(ID_RECORD, record);
    if !b {
        errors.push(format!("{} is already used by another app (recording).", record.label()));
    }
    ACTIVE.with(|c| c.set((a.then_some(shot), b.then_some(record))));
    errors
}

/// WM_HOTKEY → command.
pub fn command_for(id: i32) -> Option<Command> {
    util::log(format!("hotkey {id} pressed"));
    match id {
        ID_SHOT => Some(Command::CaptureScreenshot),
        ID_RECORD => Some(Command::RecordHotkey),
        ID_ESCAPE => Some(Command::Escape),
        _ => None,
    }
}

/// Claim Esc system-wide while a selection or countdown is on screen.
pub fn set_escape(on: bool) {
    let was = ESC.with(|c| c.get());
    if on && !was {
        let ok = register(ID_ESCAPE, Shortcut { key: vk::ESCAPE, mods: 0 });
        ESC.with(|c| c.set(ok));
        if !ok {
            util::log("couldn't claim Esc");
        }
    } else if !on && was {
        unregister(ID_ESCAPE);
        ESC.with(|c| c.set(false));
    }
}

/// Pause the shortcuts while the Settings recorder listens for new keys.
pub fn suspend() {
    unregister(ID_SHOT);
    unregister(ID_RECORD);
}

pub fn resume() {
    let (a, b) = ACTIVE.with(|c| c.get());
    if let Some(a) = a {
        register(ID_SHOT, a);
    }
    if let Some(b) = b {
        register(ID_RECORD, b);
    }
}
