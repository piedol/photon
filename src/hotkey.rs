//! Global hotkeys via Carbon (`global-hotkey`). No Accessibility permission needed.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::commands::Command;
use crate::i18n::trf;
use crate::keys::Shortcut;
use crate::util;

static SHOT_ID: AtomicU32 = AtomicU32::new(0);
static RECORD_ID: AtomicU32 = AtomicU32::new(0);
static ESC_ID: AtomicU32 = AtomicU32::new(0);

struct Registered {
    manager: GlobalHotKeyManager,
    active: Vec<HotKey>,
}

thread_local! {
    static STATE: RefCell<Option<Registered>> = const { RefCell::new(None) };
}

/// Register (or re-register) both hotkeys. Returns a message per failure.
pub fn install(shot: Shortcut, record: Shortcut) -> Vec<String> {
    let mut errors = Vec::new();
    STATE.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            match GlobalHotKeyManager::new() {
                Ok(manager) => *cell = Some(Registered { manager, active: Vec::new() }),
                Err(e) => {
                    errors.push(trf("Hotkeys unavailable: {error}", &[("error", &e.to_string())]));
                    return;
                }
            }
            GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                if event.state() != HotKeyState::Pressed {
                    return;
                }
                let id = event.id();
                util::log(format!("hotkey {id} pressed"));
                let cmd = if id == SHOT_ID.load(Ordering::Relaxed) {
                    Command::CaptureScreenshot
                } else if id == RECORD_ID.load(Ordering::Relaxed) {
                    Command::RecordHotkey
                } else if id == ESC_ID.load(Ordering::Relaxed) {
                    Command::Escape
                } else {
                    return;
                };
                util::main_async(move || crate::commands::run(cmd));
            }));
        }
        let reg = cell.as_mut().unwrap();
        let old = std::mem::take(&mut reg.active);
        let _ = reg.manager.unregister_all(&old);
        for (shortcut, slot, is_shot) in [(shot, &SHOT_ID, true), (record, &RECORD_ID, false)] {
            let label = shortcut.label();
            let Some(hk) = shortcut.hotkey() else {
                let args = [("shortcut", label.as_str())];
                errors.push(if is_shot {
                    trf("The screenshot shortcut {shortcut} isn't supported.", &args)
                } else {
                    trf("The recording shortcut {shortcut} isn't supported.", &args)
                });
                continue;
            };
            match reg.manager.register(hk) {
                Ok(()) => {
                    slot.store(hk.id(), Ordering::Relaxed);
                    reg.active.push(hk);
                }
                Err(e) => {
                    let e = e.to_string();
                    let args = [("shortcut", label.as_str()), ("error", e.as_str())];
                    errors.push(if is_shot {
                        trf("{shortcut} is already used by another app (screenshot): {error}", &args)
                    } else {
                        trf("{shortcut} is already used by another app (recording): {error}", &args)
                    });
                }
            }
        }
    });
    errors
}

/// Claim Esc system-wide while a selection overlay or countdown is up, so it
/// always cancels even if macOS didn't give Photon the keyboard.
pub fn set_escape(on: bool) {
    let esc = HotKey::new(None, global_hotkey::hotkey::Code::Escape);
    STATE.with(|cell| {
        let mut cell = cell.borrow_mut();
        let Some(reg) = cell.as_mut() else { return };
        let registered = ESC_ID.load(Ordering::Relaxed) != 0;
        if on && !registered {
            match reg.manager.register(esc) {
                Ok(()) => ESC_ID.store(esc.id(), Ordering::Relaxed),
                Err(e) => util::log(format!("couldn't claim Esc: {e}")),
            }
        } else if !on && registered {
            let _ = reg.manager.unregister(esc);
            ESC_ID.store(0, Ordering::Relaxed);
        }
    });
}

/// Temporarily stop listening (while the shortcut recorder is capturing keys).
pub fn suspend() {
    STATE.with(|cell| {
        if let Some(reg) = cell.borrow_mut().as_mut() {
            let _ = reg.manager.unregister_all(&reg.active);
        }
    });
}

pub fn resume() {
    STATE.with(|cell| {
        if let Some(reg) = cell.borrow_mut().as_mut() {
            let _ = reg.manager.register_all(&reg.active);
        }
    });
}
