//! Global hotkeys via Carbon (`global-hotkey`). No Accessibility permission needed.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::commands::Command;
use crate::keys::Shortcut;
use crate::util;

static SHOT_ID: AtomicU32 = AtomicU32::new(0);
static RECORD_ID: AtomicU32 = AtomicU32::new(0);

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
                    errors.push(format!("Hotkeys unavailable: {e}"));
                    return;
                }
            }
            GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
                if event.state() != HotKeyState::Pressed {
                    return;
                }
                let id = event.id();
                let cmd = if id == SHOT_ID.load(Ordering::Relaxed) {
                    Command::CaptureScreenshot
                } else if id == RECORD_ID.load(Ordering::Relaxed) {
                    Command::RecordHotkey
                } else {
                    return;
                };
                util::main_async(move || crate::commands::run(cmd));
            }));
        }
        let reg = cell.as_mut().unwrap();
        let old = std::mem::take(&mut reg.active);
        let _ = reg.manager.unregister_all(&old);
        for (shortcut, slot, name) in [(shot, &SHOT_ID, "screenshot"), (record, &RECORD_ID, "recording")] {
            let Some(hk) = shortcut.hotkey() else {
                errors.push(format!("The {name} shortcut {} isn't supported.", shortcut.label()));
                continue;
            };
            match reg.manager.register(hk) {
                Ok(()) => {
                    slot.store(hk.id(), Ordering::Relaxed);
                    reg.active.push(hk);
                }
                Err(e) => errors.push(format!(
                    "{} is already used by another app ({name}): {e}",
                    shortcut.label()
                )),
            }
        }
    });
    errors
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
