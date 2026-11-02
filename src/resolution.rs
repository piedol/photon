//! The size a recording is saved at. Platform-free: the Mac app and the Windows edition
//! (through `#[path]`) both use this file.

use crate::i18n::tr;

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
