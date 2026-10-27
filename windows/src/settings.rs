//! Persistent preferences in `%APPDATA%\Photon\settings.ini` (plain `key=value`),
//! loaded once and cached on the UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::keys::Shortcut;
use crate::naming::{Case, DateFormat, Kind, NamingRule, Separator, TimeFormat};
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub shot_hotkey: Shortcut,
    pub record_hotkey: Shortcut,

    pub shot_autosave: bool,
    pub shot_dir: PathBuf,
    pub last_save_dir: Option<PathBuf>,

    pub rec_dir: PathBuf,
    pub rec_also_copy: bool,
    pub rec_also_dir: Option<PathBuf>,
    pub rec_codec: Codec,
    pub rec_fps: u32,
    pub rec_system_audio: bool,
    pub rec_mic: bool,
    pub rec_mic_device: Option<String>,
    pub rec_countdown: bool,
    pub rec_cursor: bool,
    pub rec_click_rings: bool,
    pub rec_webcam: bool,
    pub rec_webcam_device: Option<String>,

    pub tool_color: usize,
    pub stroke: usize,

    pub shot_naming: NamingRule,
    pub rec_naming: NamingRule,
    pub shot_counter: u32,
    pub rec_counter: u32,

    /// Look for a new version on GitHub once a day.
    pub check_updates: bool,
    /// When the last update check got an answer, in seconds since 1970. 0 = never.
    pub last_update_check: u64,
}

fn known_folder(var: &str, fallback: &str) -> PathBuf {
    // Pictures/Videos can be redirected (e.g. into OneDrive); USERPROFILE is a safe base.
    let _ = var;
    util::home().join(fallback)
}

impl Settings {
    pub fn default_shot_dir() -> PathBuf {
        crate::cloud::user_folder("Pictures").unwrap_or_else(|| known_folder("Pictures", "Pictures")).join("Photon")
    }
    pub fn default_rec_dir() -> PathBuf {
        crate::cloud::user_folder("Videos").unwrap_or_else(|| known_folder("Videos", "Videos")).join("Photon")
    }

    fn file() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| util::home().join("AppData\\Roaming"));
        let d = base.join("Photon");
        let _ = std::fs::create_dir_all(&d);
        d.join("settings.ini")
    }

    fn load() -> Self {
        let text = std::fs::read_to_string(Self::file()).unwrap_or_default();
        let map: HashMap<String, String> = text
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.starts_with('#') || l.starts_with(';') {
                    return None;
                }
                let (k, v) = l.split_once('=')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        let s = |k: &str| map.get(k).cloned().filter(|v| !v.is_empty());
        let b = |k: &str, d: bool| s(k).map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(d);
        let i = |k: &str, d: i64| s(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(d);
        let rule = |p: &str, kind: Kind| {
            let def = NamingRule::default_for(kind);
            NamingRule {
                template: s(&format!("{p}Template"))
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| kind.template_from_store(&t))
                    .unwrap_or(def.template),
                date: s(&format!("{p}Date")).and_then(|k| DateFormat::from_key(&k)).unwrap_or(def.date),
                time: s(&format!("{p}Time")).and_then(|k| TimeFormat::from_key(&k)).unwrap_or(def.time),
                case: s(&format!("{p}Case")).and_then(|k| Case::from_key(&k)).unwrap_or(def.case),
                separator: s(&format!("{p}Sep")).and_then(|k| Separator::from_key(&k)).unwrap_or(def.separator),
            }
        };
        Settings {
            shot_hotkey: Shortcut::decode(i("shotHotkey", 0)).unwrap_or(Shortcut::SCREENSHOT),
            record_hotkey: Shortcut::decode(i("recordHotkey", 0)).unwrap_or(Shortcut::RECORD),
            shot_autosave: b("shotAutosave", false),
            shot_dir: s("shotDir").map(PathBuf::from).unwrap_or_else(Self::default_shot_dir),
            last_save_dir: s("lastSaveDir").map(PathBuf::from),
            rec_dir: s("recDir").map(PathBuf::from).unwrap_or_else(Self::default_rec_dir),
            rec_also_copy: b("recAlsoCopy", false),
            rec_also_dir: s("recAlsoDir").map(PathBuf::from),
            rec_codec: if s("recCodec").as_deref() == Some("hevc") { Codec::Hevc } else { Codec::H264 },
            rec_fps: if i("recFps", 60) == 30 { 30 } else { 60 },
            rec_system_audio: b("recSystemAudio", true),
            rec_mic: b("recMic", false),
            rec_mic_device: s("recMicDevice"),
            rec_countdown: b("recCountdown", true),
            rec_cursor: b("recCursor", true),
            rec_click_rings: b("recClickRings", true),
            rec_webcam: b("recWebcam", false),
            rec_webcam_device: s("recWebcamDevice"),
            tool_color: i("toolColor", 0).clamp(0, 7) as usize,
            stroke: i("stroke", 1).clamp(0, 2) as usize,
            shot_naming: rule("shotName", Kind::Screenshot),
            rec_naming: rule("recName", Kind::Recording),
            shot_counter: i("shotCounter", 1).max(1) as u32,
            rec_counter: i("recCounter", 1).max(1) as u32,
            check_updates: b("checkUpdates", true),
            last_update_check: i("lastUpdateCheck", 0).max(0) as u64,
        }
    }

    fn save(&self) {
        let mut out = String::from("# Photon settings. Edit with Photon closed, or use Settings.\n");
        let mut put = |k: &str, v: String| {
            out.push_str(k);
            out.push('=');
            out.push_str(&v.replace(['\n', '\r'], " "));
            out.push('\n');
        };
        let p = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        put("shotHotkey", self.shot_hotkey.encode().to_string());
        put("recordHotkey", self.record_hotkey.encode().to_string());
        put("shotAutosave", (self.shot_autosave as u8).to_string());
        put("shotDir", self.shot_dir.to_string_lossy().into_owned());
        put("lastSaveDir", p(&self.last_save_dir));
        put("recDir", self.rec_dir.to_string_lossy().into_owned());
        put("recAlsoCopy", (self.rec_also_copy as u8).to_string());
        put("recAlsoDir", p(&self.rec_also_dir));
        put("recCodec", if self.rec_codec == Codec::Hevc { "hevc" } else { "h264" }.into());
        put("recFps", self.rec_fps.to_string());
        put("recSystemAudio", (self.rec_system_audio as u8).to_string());
        put("recMic", (self.rec_mic as u8).to_string());
        put("recMicDevice", self.rec_mic_device.clone().unwrap_or_default());
        put("recCountdown", (self.rec_countdown as u8).to_string());
        put("recCursor", (self.rec_cursor as u8).to_string());
        put("recClickRings", (self.rec_click_rings as u8).to_string());
        put("recWebcam", (self.rec_webcam as u8).to_string());
        put("recWebcamDevice", self.rec_webcam_device.clone().unwrap_or_default());
        put("toolColor", self.tool_color.to_string());
        put("stroke", self.stroke.to_string());
        for (prefix, kind, r) in [
            ("shotName", Kind::Screenshot, &self.shot_naming),
            ("recName", Kind::Recording, &self.rec_naming),
        ] {
            put(&format!("{prefix}Template"), kind.template_for_store(&r.template));
            put(&format!("{prefix}Date"), r.date.key().into());
            put(&format!("{prefix}Time"), r.time.key().into());
            put(&format!("{prefix}Case"), r.case.key().into());
            put(&format!("{prefix}Sep"), r.separator.key().into());
        }
        put("shotCounter", self.shot_counter.to_string());
        put("recCounter", self.rec_counter.to_string());
        put("checkUpdates", (self.check_updates as u8).to_string());
        put("lastUpdateCheck", self.last_update_check.to_string());
        let path = Self::file();
        let tmp = path.with_extension("ini.tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// A snapshot of the current settings.
pub fn get() -> Settings {
    CURRENT.with(|c| c.borrow_mut().get_or_insert_with(Settings::load).clone())
}

/// Change settings and persist them.
pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        let s = c.get_or_insert_with(Settings::load);
        f(s);
        s.save();
        s.clone()
    })
}
