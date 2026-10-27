//! Translated text for the Windows edition, with the same calls as the Mac's `src/i18n.rs`:
//! `tr`, `trf` and `trn`, keyed by the English text. The catalogs in `locales/` are compiled
//! in, and the language follows Windows' display language (Settings › Time & language ›
//! Language & region). `PHOTON_LANG=de` overrides it, for testing.

use std::collections::HashMap;
use std::sync::OnceLock;

use windows::Win32::Globalization::GetUserDefaultUILanguage;

use crate::i18n_text::{fill, is_singular, parse_catalog};

const CATALOGS: &[(&str, &str)] = &[
    ("zh-Hans", include_str!("../../locales/zh-Hans.json")),
    ("ja", include_str!("../../locales/ja.json")),
    ("de", include_str!("../../locales/de.json")),
    ("fr", include_str!("../../locales/fr.json")),
    ("es", include_str!("../../locales/es.json")),
    ("pt-BR", include_str!("../../locales/pt-BR.json")),
    ("ko", include_str!("../../locales/ko.json")),
];

struct Active {
    lang: &'static str,
    text: HashMap<String, String>,
}

fn active() -> &'static Active {
    static ACTIVE: OnceLock<Active> = OnceLock::new();
    ACTIVE.get_or_init(|| {
        let wanted = std::env::var("PHOTON_LANG").ok().unwrap_or_else(system_language);
        let found = CATALOGS.iter().find(|(lang, _)| *lang == wanted);
        match found {
            Some((lang, json)) => Active {
                lang,
                text: parse_catalog(json).map(|c| c.into_iter().collect()).unwrap_or_default(),
            },
            None => Active { lang: "en", text: HashMap::new() },
        }
    })
}

/// Windows' display language as one of Photon's language codes, or "en".
fn system_language() -> String {
    // SAFETY: no arguments; returns the user's UI language ID.
    let id = unsafe { GetUserDefaultUILanguage() };
    let (primary, sub) = (id & 0x3ff, id >> 10);
    match primary {
        // Simplified Chinese is China (2) and Singapore (4); the others are Traditional.
        0x04 if matches!(sub, 0x02 | 0x04) => "zh-Hans",
        0x11 => "ja",
        0x07 => "de",
        0x0c => "fr",
        0x0a => "es",
        0x16 => "pt-BR",
        0x12 => "ko",
        _ => "en",
    }
    .to_string()
}

pub fn tr(key: &str) -> String {
    active().text.get(key).cloned().unwrap_or_else(|| key.to_string())
}

pub fn trf(key: &str, args: &[(&str, &str)]) -> String {
    fill(&tr(key), args)
}

pub fn trn(n: u64, one: &str, other: &str) -> String {
    let key = if is_singular(language(), n) { one } else { other };
    trf(key, &[("n", &n.to_string())])
}

pub fn language() -> &'static str {
    active().lang
}
