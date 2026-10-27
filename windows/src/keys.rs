//! Shortcuts on Windows: a virtual-key code plus RegisterHotKey modifier flags.

use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

pub const ALT: u32 = MOD_ALT.0;
pub const CONTROL: u32 = MOD_CONTROL.0;
pub const SHIFT: u32 = MOD_SHIFT.0;
pub const WIN: u32 = MOD_WIN.0;
pub const MOD_MASK: u32 = ALT | CONTROL | SHIFT | WIN;

pub mod vk {
    pub const RETURN: u16 = 0x0D;
    pub const ESCAPE: u16 = 0x1B;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const C: u16 = 0x43;
    pub const P: u16 = 0x50;
    pub const S: u16 = 0x53;
    pub const Z: u16 = 0x5A;
    pub const OEM_4: u16 = 0xDB; // [
    pub const OEM_6: u16 = 0xDD; // ]
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub key: u16,
    pub mods: u32,
}

impl Shortcut {
    /// Ctrl+Shift+9 / Ctrl+Shift+0, mirroring ⇧⌘9 / ⇧⌘0 on the Mac.
    pub const SCREENSHOT: Shortcut = Shortcut { key: 0x39, mods: CONTROL | SHIFT };
    pub const RECORD: Shortcut = Shortcut { key: 0x30, mods: CONTROL | SHIFT };

    pub fn label(self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.mods & WIN != 0 { parts.push("Win".into()); }
        if self.mods & CONTROL != 0 { parts.push(crate::i18n::tr("Ctrl")); }
        if self.mods & ALT != 0 { parts.push("Alt".into()); }
        if self.mods & SHIFT != 0 { parts.push(crate::i18n::tr("Shift")); }
        parts.push(key_label(self.key).unwrap_or_else(|| crate::i18n::trf("Key {code}", &[("code", &self.key.to_string())])));
        parts.join("+")
    }

    /// Needs Ctrl, Alt or Win, and a key that isn't itself a modifier.
    pub fn is_valid(self) -> bool {
        self.mods & (CONTROL | ALT | WIN) != 0
            && !matches!(self.key, vk::SHIFT | vk::CONTROL | vk::MENU | vk::LWIN | vk::RWIN | 0)
    }

    pub fn encode(self) -> i64 {
        ((self.mods as i64) << 16) | self.key as i64
    }

    pub fn decode(v: i64) -> Option<Self> {
        if v <= 0 {
            return None;
        }
        let s = Shortcut { key: (v & 0xffff) as u16, mods: ((v >> 16) as u32) & MOD_MASK };
        s.is_valid().then_some(s)
    }
}

pub fn key_label(vk: u16) -> Option<String> {
    Some(match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".into(),
        0x2C => "PrtScn".into(),
        0x0D => crate::i18n::tr("Enter"),
        0x09 => "Tab".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PgUp".into(),
        0x22 => "PgDn".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        for s in [Shortcut::SCREENSHOT, Shortcut::RECORD] {
            assert_eq!(Shortcut::decode(s.encode()), Some(s));
        }
        assert_eq!(Shortcut::SCREENSHOT.label(), "Ctrl+Shift+9");
    }

    #[test]
    fn shift_only_is_invalid() {
        assert!(!Shortcut { key: 0x39, mods: SHIFT }.is_valid());
    }
}
