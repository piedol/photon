//! Persistent preferences (NSUserDefaults), loaded once and cached on the main thread.

use std::cell::RefCell;
use std::path::PathBuf;

use objc2_foundation::NSUserDefaults;

use crate::keys::Shortcut;
use crate::output::naming::{Case, DateFormat, Kind, NamingRule, Separator, TimeFormat};
use crate::util::{self, ns};

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
}

fn load_rule(d: &NSUserDefaults, prefix: &str, kind: Kind) -> NamingRule {
    let def = NamingRule::default_for(kind);
    let s = |k: &str| d.stringForKey(&ns(&format!("{prefix}{k}"))).map(|v| v.to_string());
    NamingRule {
        template: s("Template")
            .filter(|t| !t.trim().is_empty())
            .map(|t| kind.template_from_store(&t))
            .unwrap_or(def.template),
        date: s("Date").and_then(|k| DateFormat::from_key(&k)).unwrap_or(def.date),
        time: s("Time").and_then(|k| TimeFormat::from_key(&k)).unwrap_or(def.time),
        case: s("Case").and_then(|k| Case::from_key(&k)).unwrap_or(def.case),
        separator: s("Sep").and_then(|k| Separator::from_key(&k)).unwrap_or(def.separator),
    }
}

fn save_rule(d: &NSUserDefaults, prefix: &str, kind: Kind, r: &NamingRule) {
    let set = |k: &str, v: &str| unsafe { d.setObject_forKey(Some(&ns(v)), &ns(&format!("{prefix}{k}"))) };
    set("Template", &kind.template_for_store(&r.template));
    set("Date", r.date.key());
    set("Time", r.time.key());
    set("Case", r.case.key());
    set("Sep", r.separator.key());
}

impl Settings {
    pub fn default_shot_dir() -> PathBuf {
        util::home().join("Pictures/Photon")
    }
    pub fn default_rec_dir() -> PathBuf {
        util::home().join("Movies/Photon")
    }

    fn load() -> Self {
        let d = NSUserDefaults::standardUserDefaults();
        let s = |k: &str| d.stringForKey(&ns(k)).map(|v| v.to_string()).filter(|v| !v.is_empty());
        let has = |k: &str| d.objectForKey(&ns(k)).is_some();
        let b = |k: &str, def: bool| if has(k) { d.boolForKey(&ns(k)) } else { def };
        let i = |k: &str, def: i64| if has(k) { d.integerForKey(&ns(k)) as i64 } else { def };
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
            // 60 fps is the default. `recFps60` replaces `recFps`, which every save wrote as 30.
            rec_fps: if i("recFps60", 60) == 30 { 30 } else { 60 },
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
            shot_naming: load_rule(&d, "shotName", Kind::Screenshot),
            rec_naming: load_rule(&d, "recName", Kind::Recording),
            shot_counter: i("shotCounter", 1).max(1) as u32,
            rec_counter: i("recCounter", 1).max(1) as u32,
        }
    }

    fn save(&self) {
        let d = NSUserDefaults::standardUserDefaults();
        let set_s = |k: &str, v: Option<&str>| unsafe {
            match v {
                Some(v) => d.setObject_forKey(Some(&ns(v)), &ns(k)),
                None => d.setObject_forKey(None, &ns(k)),
            }
        };
        let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        d.setInteger_forKey(self.shot_hotkey.encode() as isize, &ns("shotHotkey"));
        d.setInteger_forKey(self.record_hotkey.encode() as isize, &ns("recordHotkey"));
        d.setBool_forKey(self.shot_autosave, &ns("shotAutosave"));
        set_s("shotDir", Some(&self.shot_dir.to_string_lossy()));
        set_s("lastSaveDir", path(&self.last_save_dir).as_deref());
        set_s("recDir", Some(&self.rec_dir.to_string_lossy()));
        d.setBool_forKey(self.rec_also_copy, &ns("recAlsoCopy"));
        set_s("recAlsoDir", path(&self.rec_also_dir).as_deref());
        set_s("recCodec", Some(if self.rec_codec == Codec::Hevc { "hevc" } else { "h264" }));
        d.setInteger_forKey(self.rec_fps as isize, &ns("recFps60"));
        d.setBool_forKey(self.rec_system_audio, &ns("recSystemAudio"));
        d.setBool_forKey(self.rec_mic, &ns("recMic"));
        set_s("recMicDevice", self.rec_mic_device.as_deref());
        d.setBool_forKey(self.rec_countdown, &ns("recCountdown"));
        d.setBool_forKey(self.rec_cursor, &ns("recCursor"));
        d.setBool_forKey(self.rec_click_rings, &ns("recClickRings"));
        d.setBool_forKey(self.rec_webcam, &ns("recWebcam"));
        set_s("recWebcamDevice", self.rec_webcam_device.as_deref());
        d.setInteger_forKey(self.tool_color as isize, &ns("toolColor"));
        d.setInteger_forKey(self.stroke as isize, &ns("stroke"));
        save_rule(&d, "shotName", Kind::Screenshot, &self.shot_naming);
        save_rule(&d, "recName", Kind::Recording, &self.rec_naming);
        d.setInteger_forKey(self.shot_counter as isize, &ns("shotCounter"));
        d.setInteger_forKey(self.rec_counter as isize, &ns("recCounter"));
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
