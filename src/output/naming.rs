//! File-name templates for auto-saved screenshots and recordings.
//!
//! A [`NamingRule`] holds a template such as `"{type} {date} at {time}"` plus
//! formatting choices. [`render_stem`] turns it into a safe file stem and
//! [`next_available`] picks a non-colliding path in a folder.

use std::path::{Path, PathBuf};

use crate::i18n::tr;

/// Longest stem we produce, in bytes (macOS allows 255 per component).
const MAX_STEM_BYTES: usize = 200;

/// Declares a UI enum with `ALL`, `label`, `key` and `from_key`.
macro_rules! keyed_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $key:literal, $label:literal;)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        #[allow(dead_code)]
        impl $name {
            /// Every variant, in UI order.
            pub const ALL: [Self; [$($name::$variant),+].len()] = [$(Self::$variant),+];

            /// The English label: `{type}` renders it in file names, the date and time
            /// popups show it as an example. Popups of words use `ui_label` instead.
            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }

            /// Stable snake_case key for persistence.
            pub fn key(self) -> &'static str {
                match self { $(Self::$variant => $key),+ }
            }

            /// Parses a key produced by [`Self::key`].
            pub fn from_key(key: &str) -> Option<Self> {
                match key { $($key => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

keyed_enum! {
    /// What is being saved; `{type}` renders the label.
    Kind {
        Screenshot => "screenshot", "Screenshot";
        Recording => "recording", "Recording";
    }
}

keyed_enum! {
    /// How `{date}` renders. Labels are examples.
    DateFormat {
        Iso => "iso", "2026-10-24";
        MonthDayYear => "month_day_year", "Oct 24 2026";
        DayMonthYear => "day_month_year", "24-10-2026";
        Compact => "compact", "20261024";
    }
}

keyed_enum! {
    /// How `{time}` renders. Labels are examples.
    TimeFormat {
        Dots => "dots", "09.41.12";
        Dashes => "dashes", "09-41-12";
        Compact => "compact", "0941";
        TwelveHour => "twelve_hour", "9.41 AM";
    }
}

keyed_enum! {
    /// Letter case applied to the whole rendered name.
    Case {
        AsTyped => "as_typed", "As typed";
        Title => "title", "Title Case";
        Lower => "lower", "lowercase";
        Upper => "upper", "UPPERCASE";
    }
}

keyed_enum! {
    /// Character placed between words.
    Separator {
        Space => "space", "Space";
        Dash => "dash", "Dash  -";
        Underscore => "underscore", "Underscore  _";
    }
}

impl Case {
    /// Text for the UI popup, in the user's language.
    pub fn ui_label(self) -> String {
        match self {
            Self::AsTyped => tr("As typed"),
            Self::Title => tr("Title Case"),
            Self::Lower => tr("lowercase"),
            Self::Upper => tr("UPPERCASE"),
        }
    }
}

impl Separator {
    /// Text for the UI popup, in the user's language.
    pub fn ui_label(self) -> String {
        match self {
            Self::Space => tr("Space"),
            Self::Dash => tr("Dash  -"),
            Self::Underscore => tr("Underscore  _"),
        }
    }

    fn as_char(self) -> char {
        match self {
            Self::Space => ' ',
            Self::Dash => '-',
            Self::Underscore => '_',
        }
    }

    /// Collision suffix for the `n`th copy: ` (2)`, `-2` or `_2`.
    fn copy_suffix(self, n: u32) -> String {
        match self {
            Self::Space => format!(" ({n})"),
            Self::Dash => format!("-{n}"),
            Self::Underscore => format!("_{n}"),
        }
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl DateFormat {
    /// Renders the date part of `t`.
    pub fn render(self, t: &LocalTime) -> String {
        let (y, m, d) = (t.year, t.month, t.day);
        match self {
            Self::Iso => format!("{y:04}-{m:02}-{d:02}"),
            Self::MonthDayYear => {
                let month = m
                    .checked_sub(1)
                    .and_then(|i| MONTHS.get(i as usize))
                    .copied()
                    .unwrap_or("???");
                format!("{month} {d} {y:04}")
            }
            Self::DayMonthYear => format!("{d:02}-{m:02}-{y:04}"),
            Self::Compact => format!("{y:04}{m:02}{d:02}"),
        }
    }
}

impl TimeFormat {
    /// Renders the time part of `t`.
    pub fn render(self, t: &LocalTime) -> String {
        let (h, m, s) = (t.hour, t.minute, t.second);
        match self {
            Self::Dots => format!("{h:02}.{m:02}.{s:02}"),
            Self::Dashes => format!("{h:02}-{m:02}-{s:02}"),
            Self::Compact => format!("{h:02}{m:02}"),
            Self::TwelveHour => {
                let suffix = if h % 24 < 12 { "AM" } else { "PM" };
                let h12 = match h % 12 {
                    0 => 12,
                    other => other,
                };
                format!("{h12}.{m:02} {suffix}")
            }
        }
    }
}

/// A user's naming preferences for one [`Kind`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamingRule {
    pub template: String,
    pub date: DateFormat,
    pub time: TimeFormat,
    pub case: Case,
    pub separator: Separator,
}

impl Kind {
    /// The word `{type}` writes into file names, in the app's language.
    pub fn file_word(self) -> String {
        match self {
            Kind::Screenshot => tr("Screenshot"),
            Kind::Recording => tr("Recording"),
        }
    }

    /// The default template as settings store it: always the English text, so a saved
    /// default follows the app's language.
    fn stored_default(self) -> &'static str {
        match self {
            Kind::Screenshot => "{type} {date} at {time}",
            Kind::Recording => "Photon {type} {date} at {time}",
        }
    }

    /// The default template in the app's language.
    pub fn default_template(self) -> String {
        match self {
            Kind::Screenshot => tr("{type} {date} at {time}"),
            Kind::Recording => tr("Photon {type} {date} at {time}"),
        }
    }

    /// A template read from settings: the stored default becomes the default in the app's language.
    pub fn template_from_store(self, stored: &str) -> String {
        if stored == self.stored_default() { self.default_template() } else { stored.to_owned() }
    }

    /// The form to store a template in: the default, in any language, is stored as the English.
    pub fn template_for_store(self, template: &str) -> String {
        if template == self.default_template() { self.stored_default().to_owned() } else { template.to_owned() }
    }
}

impl NamingRule {
    /// The built-in rule, e.g. "Screenshot 2026-10-24 at 09.41.12" in English.
    pub fn default_for(kind: Kind) -> Self {
        Self {
            template: kind.default_template(),
            date: DateFormat::Iso,
            time: TimeFormat::Dots,
            case: Case::AsTyped,
            separator: Separator::Space,
        }
    }
}

/// A broken-down local wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    /// The current local time. Falls back to 1970-01-01 00:00:00 if the
    /// system clock cannot be converted.
    pub fn now() -> Self {
        // SAFETY: `time` accepts a null out-pointer, and `localtime_r` writes
        // only into the `tm` we own. A zeroed `tm` is valid (null `tm_zone`).
        let tm = unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                None
            } else {
                Some(tm)
            }
        };
        let Some(tm) = tm else {
            return Self {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            };
        };
        let field = |v: libc::c_int| u32::try_from(v).unwrap_or(0);
        Self {
            year: tm.tm_year + 1900,
            month: field(tm.tm_mon + 1),
            day: field(tm.tm_mday),
            hour: field(tm.tm_hour),
            minute: field(tm.tm_min),
            // tm_sec can be 60 on a leap second; clamp so names stay sane.
            second: field(tm.tm_sec).min(59),
        }
    }
}

/// Values available to a template when a capture is saved.
#[derive(Clone, Debug)]
pub struct NameContext {
    pub kind: Kind,
    pub when: LocalTime,
    /// Front app name, if known.
    pub app: Option<String>,
    /// Pixel size of the capture.
    pub size: Option<(u32, u32)>,
    /// Display name, if known.
    pub display: Option<String>,
    /// Running number for `{counter}`.
    pub counter: u32,
}

/// Returns a token's short description in the user's language.
pub type Describe = fn() -> String;

/// Template tokens for UI chips, with their descriptions. The tokens themselves are
/// part of the saved templates, so they are never translated.
pub const TOKENS: [(&str, Describe); 7] = [
    ("{type}", || tr("Screenshot or Recording")),
    ("{date}", || tr("Date")),
    ("{time}", || tr("Time")),
    ("{app}", || tr("Front app")),
    ("{size}", || tr("Size, e.g. 1440x900")),
    ("{display}", || tr("Display name")),
    ("{counter}", || tr("Running number")),
];

/// Renders the file stem (no extension) for `ctx`.
///
/// Tokens are case-insensitive and unknown `{tokens}` stay literal. Words are
/// joined with the rule's separator, cased, then sanitized for the file
/// system. An empty result falls back to the default template for `ctx.kind`
/// with the same formats, so the stem is never empty.
pub fn render_stem(rule: &NamingRule, ctx: &NameContext) -> String {
    let stem = render_template(&rule.template, rule, ctx);
    if !stem.is_empty() {
        return stem;
    }
    let fallback = NamingRule::default_for(ctx.kind).template;
    let stem = render_template(&fallback, rule, ctx);
    if stem.is_empty() {
        // Unreachable in practice: `{type}` always renders non-empty.
        ctx.kind.file_word()
    } else {
        stem
    }
}

/// True when `template` contains `{counter}` (any case).
pub fn template_uses_counter(template: &str) -> bool {
    template.to_ascii_lowercase().contains("{counter}")
}

/// Picks a path in `dir` that does not exist yet, returning it with the
/// counter used.
///
/// With `{counter}` in the template, counts up from `ctx.counter`. Otherwise
/// appends ` (2)`, `-2` or `_2` (by separator) on collision and returns
/// `ctx.counter` unchanged. `ext` is given without a dot, e.g. `"png"`.
pub fn next_available(
    dir: &Path,
    rule: &NamingRule,
    ctx: &NameContext,
    ext: &str,
) -> (PathBuf, u32) {
    let ext = ext.trim_start_matches('.');
    if template_uses_counter(&rule.template) {
        let mut ctx = ctx.clone();
        loop {
            let path = dir.join(file_name(&render_stem(rule, &ctx), ext));
            if !is_taken(&path) {
                return (path, ctx.counter);
            }
            match ctx.counter.checked_add(1) {
                Some(next) => ctx.counter = next,
                // Counter exhausted; fall through to suffixing this name.
                None => break,
            }
        }
        return (
            suffixed(dir, &render_stem(rule, &ctx), rule.separator, ext),
            ctx.counter,
        );
    }
    (
        suffixed(dir, &render_stem(rule, ctx), rule.separator, ext),
        ctx.counter,
    )
}

/// `dir/stem.ext`, or the first free `dir/stem<suffix n>.ext`.
fn suffixed(dir: &Path, stem: &str, separator: Separator, ext: &str) -> PathBuf {
    let path = dir.join(file_name(stem, ext));
    if !is_taken(&path) {
        return path;
    }
    let mut n: u32 = 2;
    loop {
        let candidate = dir.join(file_name(
            &format!("{stem}{}", separator.copy_suffix(n)),
            ext,
        ));
        if !is_taken(&candidate) || n == u32::MAX {
            return candidate;
        }
        n += 1;
    }
}

fn file_name(stem: &str, ext: &str) -> String {
    if ext.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{ext}")
    }
}

/// Counts broken symlinks as taken, unlike `Path::exists`.
fn is_taken(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn render_template(template: &str, rule: &NamingRule, ctx: &NameContext) -> String {
    let expanded = expand_tokens(template, rule, ctx);
    let sep = rule.separator.as_char();
    let joined = expanded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(sep.encode_utf8(&mut [0; 4]));
    sanitize(&apply_case(&joined, rule.case), sep)
}

/// Replaces known `{token}`s in one pass; token values are not re-expanded.
fn expand_tokens(template: &str, rule: &NamingRule, ctx: &NameContext) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find(['{', '}']) {
            Some(close) if after[close..].starts_with('}') => {
                let name = &after[..close];
                match token_value(name, rule, ctx) {
                    Some(value) => out.push_str(&value),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            // Unclosed, or another `{` comes first: keep this brace literally.
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn token_value(name: &str, rule: &NamingRule, ctx: &NameContext) -> Option<String> {
    let value = match name.to_ascii_lowercase().as_str() {
        "type" => ctx.kind.file_word(),
        "date" => rule.date.render(&ctx.when),
        "time" => rule.time.render(&ctx.when),
        "app" => ctx.app.clone().unwrap_or_default(),
        "size" => ctx
            .size
            .map(|(w, h)| format!("{w}x{h}"))
            .unwrap_or_default(),
        "display" => ctx.display.clone().unwrap_or_default(),
        "counter" => format!("{:03}", ctx.counter),
        _ => return None,
    };
    Some(value)
}

fn apply_case(s: &str, case: Case) -> String {
    match case {
        Case::AsTyped => s.to_owned(),
        Case::Lower => s.to_lowercase(),
        Case::Upper => s.to_uppercase(),
        Case::Title => {
            let mut out = String::with_capacity(s.len());
            let mut word_start = true;
            for c in s.chars() {
                if word_start {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                word_start = is_word_break(c);
            }
            out
        }
    }
}

fn is_word_break(c: char) -> bool {
    c.is_whitespace() || c == '-' || c == '_'
}

/// Makes `s` safe as a macOS file stem.
fn sanitize(s: &str, sep: char) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if matches!(c, '/' | ':' | '\\') {
            '-'
        } else {
            c
        };
        if c.is_control() || (c == sep && out.ends_with(sep)) {
            continue;
        }
        out.push(c);
    }
    let trimmed = trim_edges(&out);
    let mut cut = trimmed.len().min(MAX_STEM_BYTES);
    while !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    trim_edges(&trimmed[..cut]).to_owned()
}

/// Strips leading dots, and whitespace, `-` and `_` from both ends.
fn trim_edges(s: &str) -> &str {
    s.trim_start_matches(|c: char| c == '.' || is_word_break(c))
        .trim_end_matches(is_word_break)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const WHEN: LocalTime = LocalTime {
        year: 2026,
        month: 10,
        day: 24,
        hour: 9,
        minute: 41,
        second: 12,
    };

    fn ctx(kind: Kind) -> NameContext {
        NameContext {
            kind,
            when: WHEN,
            app: None,
            size: None,
            display: None,
            counter: 1,
        }
    }

    fn rule(template: &str) -> NamingRule {
        NamingRule {
            template: template.to_owned(),
            ..NamingRule::default_for(Kind::Screenshot)
        }
    }

    fn at(hour: u32, minute: u32) -> LocalTime {
        LocalTime {
            hour,
            minute,
            second: 0,
            ..WHEN
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let name = format!(
                "photon-naming-{}-{nanos}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            );
            let dir = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn touch(&self, name: &str) {
            std::fs::write(self.0.join(name), b"").expect("touch");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn defaults() {
        let shot = NamingRule::default_for(Kind::Screenshot);
        assert_eq!(
            render_stem(&shot, &ctx(Kind::Screenshot)),
            "Screenshot 2026-10-24 at 09.41.12"
        );
        let rec = NamingRule::default_for(Kind::Recording);
        assert_eq!(
            render_stem(&rec, &ctx(Kind::Recording)),
            "Photon Recording 2026-10-24 at 09.41.12"
        );
    }

    #[test]
    fn every_token() {
        let mut c = ctx(Kind::Recording);
        c.app = Some("Safari".into());
        c.size = Some((1440, 900));
        c.display = Some("Built-in".into());
        c.counter = 7;
        let r = rule("{type}|{date}|{time}|{app}|{size}|{display}|{counter}");
        assert_eq!(
            render_stem(&r, &c),
            "Recording|2026-10-24|09.41.12|Safari|1440x900|Built-in|007"
        );
    }

    #[test]
    fn tokens_case_insensitive_and_unknown_literal() {
        let c = ctx(Kind::Screenshot);
        assert_eq!(
            render_stem(&rule("{Date} {TYPE}"), &c),
            "2026-10-24 Screenshot"
        );
        assert_eq!(render_stem(&rule("{foo} {date}"), &c), "{foo} 2026-10-24");
        assert_eq!(render_stem(&rule("{{date} {x"), &c), "{2026-10-24 {x");
    }

    #[test]
    fn token_values_not_reexpanded() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("{date}".into());
        assert_eq!(render_stem(&rule("{app}"), &c), "{date}");
    }

    #[test]
    fn optional_tokens_empty_when_none() {
        let c = ctx(Kind::Screenshot);
        assert_eq!(
            render_stem(&rule("{type} {app} {size} {display} x"), &c),
            "Screenshot x"
        );
    }

    #[test]
    fn counter_padding() {
        let mut c = ctx(Kind::Screenshot);
        let r = rule("{counter}");
        for (n, want) in [
            (0, "000"),
            (7, "007"),
            (42, "042"),
            (999, "999"),
            (1000, "1000"),
            (123456, "123456"),
        ] {
            c.counter = n;
            assert_eq!(render_stem(&r, &c), want);
        }
    }

    #[test]
    fn date_formats() {
        let mut c = ctx(Kind::Screenshot);
        let mut r = rule("{date}");
        for (fmt, want) in [
            (DateFormat::Iso, "2026-10-24"),
            (DateFormat::MonthDayYear, "Oct 24 2026"),
            (DateFormat::DayMonthYear, "24-10-2026"),
            (DateFormat::Compact, "20261024"),
        ] {
            r.date = fmt;
            assert_eq!(render_stem(&r, &c), want);
            assert_eq!(fmt.label(), want);
        }
        c.when = LocalTime {
            month: 3,
            day: 5,
            ..WHEN
        };
        assert_eq!(DateFormat::MonthDayYear.render(&c.when), "Mar 5 2026");
        assert_eq!(DateFormat::Iso.render(&c.when), "2026-03-05");
        c.when.month = 13;
        assert_eq!(DateFormat::MonthDayYear.render(&c.when), "??? 5 2026");
    }

    #[test]
    fn time_formats() {
        for (fmt, want) in [
            (TimeFormat::Dots, "09.41.12"),
            (TimeFormat::Dashes, "09-41-12"),
            (TimeFormat::Compact, "0941"),
            (TimeFormat::TwelveHour, "9.41 AM"),
        ] {
            assert_eq!(fmt.render(&WHEN), want);
            assert_eq!(fmt.label(), want);
        }
        let twelve = |h, m| TimeFormat::TwelveHour.render(&at(h, m));
        assert_eq!(twelve(0, 5), "12.05 AM");
        assert_eq!(twelve(12, 0), "12.00 PM");
        assert_eq!(twelve(13, 0), "1.00 PM");
        assert_eq!(twelve(23, 59), "11.59 PM");
    }

    #[test]
    fn twelve_hour_through_separator() {
        let mut r = rule("{time}");
        r.time = TimeFormat::TwelveHour;
        r.separator = Separator::Underscore;
        assert_eq!(render_stem(&r, &ctx(Kind::Screenshot)), "9.41_AM");
    }

    #[test]
    fn case_and_separator_matrix() {
        use Case::*;
        use Separator::*;
        let c = ctx(Kind::Recording);
        let cases = [
            (AsTyped, Space, "Photon Recording 2026-10-24 at 09.41.12"),
            (AsTyped, Dash, "Photon-Recording-2026-10-24-at-09.41.12"),
            (
                AsTyped,
                Underscore,
                "Photon_Recording_2026-10-24_at_09.41.12",
            ),
            (Title, Space, "Photon Recording 2026-10-24 At 09.41.12"),
            (Title, Dash, "Photon-Recording-2026-10-24-At-09.41.12"),
            (Title, Underscore, "Photon_Recording_2026-10-24_At_09.41.12"),
            (Lower, Space, "photon recording 2026-10-24 at 09.41.12"),
            (Lower, Dash, "photon-recording-2026-10-24-at-09.41.12"),
            (Lower, Underscore, "photon_recording_2026-10-24_at_09.41.12"),
            (Upper, Space, "PHOTON RECORDING 2026-10-24 AT 09.41.12"),
            (Upper, Dash, "PHOTON-RECORDING-2026-10-24-AT-09.41.12"),
            (Upper, Underscore, "PHOTON_RECORDING_2026-10-24_AT_09.41.12"),
        ];
        assert_eq!(cases.len(), Case::ALL.len() * Separator::ALL.len());
        for (case, separator, want) in cases {
            let r = NamingRule {
                case,
                separator,
                ..NamingRule::default_for(Kind::Recording)
            };
            assert_eq!(render_stem(&r, &c), want, "{case:?} {separator:?}");
        }
    }

    #[test]
    fn title_case_keeps_rest_of_word() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("visual studio code".into());
        let mut r = rule("{app} at {time} e-mail_x");
        r.case = Case::Title;
        r.time = TimeFormat::TwelveHour;
        assert_eq!(
            render_stem(&r, &c),
            "Visual Studio Code At 9.41 AM E-Mail_X"
        );
        r.separator = Separator::Dash;
        r.date = DateFormat::MonthDayYear;
        r.template = "{date}".into();
        assert_eq!(render_stem(&r, &c), "Oct-24-2026");
    }

    #[test]
    fn separator_splits_token_values() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("  Visual   Studio Code ".into());
        let mut r = rule("{app}  {date}");
        r.separator = Separator::Dash;
        r.date = DateFormat::MonthDayYear;
        assert_eq!(render_stem(&r, &c), "Visual-Studio-Code-Oct-24-2026");
    }

    #[test]
    fn unicode_app_names() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("Café".into());
        let mut r = rule("{app} {type}");
        assert_eq!(render_stem(&r, &c), "Café Screenshot");
        r.case = Case::Upper;
        assert_eq!(render_stem(&r, &c), "CAFÉ SCREENSHOT");
        c.app = Some("éclair 日本".into());
        r.case = Case::Title;
        assert_eq!(render_stem(&r, &c), "Éclair 日本 Screenshot");
    }

    #[test]
    fn unsafe_characters() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("AC/DC: Live\\Tour".into());
        let mut r = rule("{app}");
        assert_eq!(render_stem(&r, &c), "AC-DC- Live-Tour");
        r.separator = Separator::Dash;
        assert_eq!(render_stem(&r, &c), "AC-DC-Live-Tour");

        c.app = Some("a\u{7}b\u{0}c\u{7f}".into());
        assert_eq!(render_stem(&rule("{app}"), &c), "abc");

        c.app = Some("..hidden".into());
        assert_eq!(render_stem(&rule("{app}"), &c), "hidden");
        assert_eq!(render_stem(&rule(" - _ /x/ - "), &c), "x");
        assert_eq!(
            render_stem(&rule("{type} - {app}"), &ctx(Kind::Screenshot)),
            "Screenshot"
        );

        let mut dashed = rule("a - - b");
        dashed.separator = Separator::Dash;
        assert_eq!(render_stem(&dashed, &c), "a-b");
    }

    #[test]
    fn empty_falls_back_to_default() {
        let mut r = rule("{app}");
        r.separator = Separator::Dash;
        r.case = Case::Lower;
        assert_eq!(
            render_stem(&r, &ctx(Kind::Screenshot)),
            "screenshot-2026-10-24-at-09.41.12"
        );
        assert_eq!(
            render_stem(&rule(""), &ctx(Kind::Recording)),
            "Photon Recording 2026-10-24 at 09.41.12"
        );
        assert_eq!(
            render_stem(&rule("..."), &ctx(Kind::Screenshot)),
            "Screenshot 2026-10-24 at 09.41.12"
        );
    }

    #[test]
    fn truncates_to_200_bytes_on_char_boundary() {
        let mut c = ctx(Kind::Screenshot);
        c.app = Some("é".repeat(150));
        let out = render_stem(&rule("{app}"), &c);
        assert_eq!(out.len(), 200);
        assert_eq!(out, "é".repeat(100));

        c.app = Some(format!("a{}", "é".repeat(150)));
        let out = render_stem(&rule("{app}"), &c);
        assert_eq!(out.len(), 199);

        // Trailing separator exposed by the cut is trimmed.
        c.app = Some(format!("{} {}", "a".repeat(199), "b".repeat(10)));
        let out = render_stem(&rule("{app}"), &c);
        assert_eq!(out, "a".repeat(199));
    }

    #[test]
    fn uses_counter_detection() {
        assert!(template_uses_counter("Shot {counter}"));
        assert!(template_uses_counter("Shot {Counter}"));
        assert!(!template_uses_counter("Shot {count}"));
    }

    #[test]
    fn keys_round_trip() {
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            key: fn(T) -> &'static str,
            from: fn(&str) -> Option<T>,
        ) {
            for &v in all {
                assert_eq!(from(key(v)), Some(v));
                assert!(key(v).chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            }
            assert_eq!(from("nope"), None);
        }
        check(&Kind::ALL, Kind::key, Kind::from_key);
        check(&DateFormat::ALL, DateFormat::key, DateFormat::from_key);
        check(&TimeFormat::ALL, TimeFormat::key, TimeFormat::from_key);
        check(&Case::ALL, Case::key, Case::from_key);
        check(&Separator::ALL, Separator::key, Separator::from_key);
        assert_eq!(Case::Title.label(), "Title Case");
        assert_eq!(Separator::Underscore.label(), "Underscore  _");
        assert_eq!(Kind::Recording.label(), "Recording");
    }

    #[test]
    fn now_is_plausible() {
        let t = LocalTime::now();
        assert!(t.year >= 2024);
        assert!((1..=12).contains(&t.month) && (1..=31).contains(&t.day));
        assert!(t.hour < 24 && t.minute < 60 && t.second < 60);
    }

    #[test]
    fn collisions_append_suffix() {
        let dir = TempDir::new();
        let c = ctx(Kind::Screenshot);
        let mut r = NamingRule::default_for(Kind::Screenshot);

        let (p, n) = next_available(&dir.0, &r, &c, "png");
        assert_eq!(p, dir.0.join("Screenshot 2026-10-24 at 09.41.12.png"));
        assert_eq!(n, 1);

        dir.touch("Screenshot 2026-10-24 at 09.41.12.png");
        let (p, _) = next_available(&dir.0, &r, &c, "png");
        assert_eq!(p, dir.0.join("Screenshot 2026-10-24 at 09.41.12 (2).png"));
        dir.touch("Screenshot 2026-10-24 at 09.41.12 (2).png");
        let (p, n) = next_available(&dir.0, &r, &c, "png");
        assert_eq!(p, dir.0.join("Screenshot 2026-10-24 at 09.41.12 (3).png"));
        assert_eq!(n, 1);

        // Different extension does not collide.
        let (p, _) = next_available(&dir.0, &r, &c, "mov");
        assert_eq!(p, dir.0.join("Screenshot 2026-10-24 at 09.41.12.mov"));

        r.separator = Separator::Dash;
        dir.touch("Screenshot-2026-10-24-at-09.41.12.png");
        let (p, _) = next_available(&dir.0, &r, &c, "png");
        assert_eq!(p, dir.0.join("Screenshot-2026-10-24-at-09.41.12-2.png"));

        r.separator = Separator::Underscore;
        dir.touch("Screenshot_2026-10-24_at_09.41.12.png");
        let (p, _) = next_available(&dir.0, &r, &c, "png");
        assert_eq!(p, dir.0.join("Screenshot_2026-10-24_at_09.41.12_2.png"));
    }

    #[test]
    fn counter_increments_past_existing() {
        let dir = TempDir::new();
        let r = rule("Shot {counter}");
        let mut c = ctx(Kind::Screenshot);
        let (p, n) = next_available(&dir.0, &r, &c, "png");
        assert_eq!((p, n), (dir.0.join("Shot 001.png"), 1));

        dir.touch("Shot 001.png");
        dir.touch("Shot 002.png");
        let (p, n) = next_available(&dir.0, &r, &c, "png");
        assert_eq!((p, n), (dir.0.join("Shot 003.png"), 3));

        c.counter = 5;
        let (p, n) = next_available(&dir.0, &r, &c, "png");
        assert_eq!((p, n), (dir.0.join("Shot 005.png"), 5));
    }
}
