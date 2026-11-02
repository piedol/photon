//! Persistent preferences (NSUserDefaults), loaded once and cached on the main thread.

use std::cell::RefCell;
use std::path::PathBuf;

use objc2_foundation::NSUserDefaults;

use crate::i18n::tr;
use crate::keys::Shortcut;
use crate::output::naming::{Case, DateFormat, Kind, NamingRule, Separator, TimeFormat};
use crate::util::{self, ns};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    H264,
    Hevc,
}

/// The largest size a recording is saved at. The video is scaled down to fit inside the
/// preset, keeping its shape, and never scaled up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Native,
    P2160,
    P1440,
    P1080,
    P720,
}

impl Resolution {
    pub const ALL: [Resolution; 5] = [Resolution::Native, Resolution::P2160, Resolution::P1440, Resolution::P1080, Resolution::P720];

    pub fn key(self) -> &'static str {
        match self {
            Resolution::Native => "native",
            Resolution::P2160 => "2160",
            Resolution::P1440 => "1440",
            Resolution::P1080 => "1080",
            Resolution::P720 => "720",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == k)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|r| *r == self).unwrap_or(0)
    }

    /// The name in Settings and in the record bar's list.
    pub fn label(self) -> String {
        match self {
            Resolution::Native => tr("Native"),
            Resolution::P2160 => "4K (2160p)".into(),
            Resolution::P1440 => "1440p".into(),
            Resolution::P1080 => "1080p".into(),
            Resolution::P720 => "720p".into(),
        }
    }

    /// The short name on the record bar.
    pub fn short_label(self) -> String {
        match self {
            Resolution::P2160 => "4K".into(),
            r => r.label(),
        }
    }

    /// The landscape box the video must fit in, as (long side, short side).
    fn bounds(self) -> Option<(f64, f64)> {
        match self {
            Resolution::Native => None,
            Resolution::P2160 => Some((3840.0, 2160.0)),
            Resolution::P1440 => Some((2560.0, 1440.0)),
            Resolution::P1080 => Some((1920.0, 1080.0)),
            Resolution::P720 => Some((1280.0, 720.0)),
        }
    }

    /// The saved size for an area of `w × h` pixels: even numbers, as the encoders need.
    /// The long side fits the preset's long side and the short side its short side, so a
    /// portrait area at 1080p is at most 1080 wide and 1920 tall.
    pub fn fit(self, w: f64, h: f64) -> (usize, usize) {
        let even_up = |v: f64| ((v.round() as usize) + 1) & !1;
        let k = match self.bounds() {
            Some((long, short)) => (long / w.max(h)).min(short / w.min(h)).min(1.0),
            None => 1.0,
        };
        if k >= 1.0 {
            return (even_up(w).max(2), even_up(h).max(2));
        }
        // Scaled down: round down to even so the result stays inside the preset.
        let even_down = |v: f64| ((v.round() as usize) & !1).max(2);
        (even_down(w * k), even_down(h * k))
    }
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
    /// The default for each recording; the record bar can change it for one recording.
    pub rec_resolution: Resolution,
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
            rec_resolution: s("recResolution").and_then(|k| Resolution::from_key(&k)).unwrap_or(Resolution::Native),
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
            check_updates: b("checkUpdates", true),
            last_update_check: i("lastUpdateCheck", 0).max(0) as u64,
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
        set_s("recResolution", Some(self.rec_resolution.key()));
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
        d.setBool_forKey(self.check_updates, &ns("checkUpdates"));
        d.setInteger_forKey(self.last_update_check as isize, &ns("lastUpdateCheck"));
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

#[cfg(test)]
mod tests {
    use super::Resolution::{self, *};

    #[test]
    fn native_keeps_the_size() {
        assert_eq!(Native.fit(3456.0, 2234.0), (3456, 2234));
        assert_eq!(Native.fit(1001.0, 601.0), (1002, 602));
    }

    #[test]
    fn presets_fit_inside_and_keep_the_shape() {
        // A full 16:10 Retina screen.
        assert_eq!(P1080.fit(3456.0, 2234.0), (1670, 1080));
        assert_eq!(P1440.fit(3456.0, 2234.0), (2228, 1440));
        assert_eq!(P720.fit(3456.0, 2234.0), (1114, 720));
        // A 16:9 area lands on the preset exactly.
        assert_eq!(P1080.fit(3840.0, 2160.0), (1920, 1080));
        // A wide strip is limited by its width.
        assert_eq!(P1080.fit(4000.0, 1000.0), (1920, 480));
        // Portrait.
        assert_eq!(P1080.fit(2000.0, 4000.0), (960, 1920));
    }

    #[test]
    fn small_areas_are_never_scaled_up() {
        assert_eq!(P1080.fit(800.0, 600.0), (800, 600));
        assert_eq!(P2160.fit(3024.0, 1964.0), (3024, 1964));
        // 4K still limits a 16:10 screen taller than 2160 pixels.
        assert_eq!(P2160.fit(3456.0, 2234.0), (3342, 2160));
    }

    #[test]
    fn keys_round_trip() {
        for r in Resolution::ALL {
            assert_eq!(Resolution::from_key(r.key()), Some(r));
            assert_eq!(Resolution::ALL[r.index()], r);
        }
        assert_eq!(Resolution::from_key("8k"), None);
    }
}
