//! The platform-free half of translation, shared by the Mac and Windows editions: the
//! languages Photon ships, `{name}` placeholders, plural choice and the catalog format.
//!
//! A catalog is `locales/<lang>.json`, a flat JSON object from the English text to its
//! translation. English has no catalog: the English text is the key.

/// Every language with a catalog, as macOS names its `.lproj` folders.
#[allow(dead_code)] // read by the catalog test
pub const LANGUAGES: &[&str] = &["zh-Hans", "ja", "de", "fr", "es", "pt-BR", "ko"];

/// Replace each `{name}` in `text` with its value.
pub fn fill(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// Whether `n` takes the singular form in `lang`. French and Portuguese use it for 0 and 1;
/// Chinese, Japanese and Korean have no plural form, so they always take the other one.
pub fn is_singular(lang: &str, n: u64) -> bool {
    match lang {
        "zh-Hans" | "ja" | "ko" => false,
        "fr" | "pt-BR" => n <= 1,
        _ => n == 1,
    }
}

/// Parse a catalog: a flat JSON object whose keys and values are all strings.
#[allow(dead_code)] // the Windows edition's compiled-in catalogs; the Mac reads them through NSBundle
pub fn parse_catalog(json: &str) -> Result<Vec<(String, String)>, String> {
    let mut p = Parser { s: json.as_bytes(), i: 0 };
    let mut out = Vec::new();
    p.ws();
    p.expect(b'{')?;
    p.ws();
    if p.peek() == Some(b'}') {
        return Ok(out);
    }
    loop {
        p.ws();
        let key = p.string()?;
        p.ws();
        p.expect(b':')?;
        p.ws();
        let value = p.string()?;
        out.push((key, value));
        p.ws();
        match p.next() {
            Some(b',') => continue,
            Some(b'}') => return Ok(out),
            _ => return Err(format!("expected , or }} at byte {}", p.i)),
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let c = self.peek();
        self.i += 1;
        c
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.next() == Some(c) {
            Ok(())
        } else {
            Err(format!("expected {} at byte {}", c as char, self.i - 1))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            match self.next() {
                None => return Err("unterminated string".into()),
                Some(b'"') => break,
                Some(b'\\') => match self.next() {
                    Some(b'"') => bytes.push(b'"'),
                    Some(b'\\') => bytes.push(b'\\'),
                    Some(b'/') => bytes.push(b'/'),
                    Some(b'n') => bytes.push(b'\n'),
                    Some(b't') => bytes.push(b'\t'),
                    Some(b'r') => bytes.push(b'\r'),
                    Some(b'u') => {
                        let hex = self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?;
                        let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                        let code = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                        self.i += 4;
                        let c = char::from_u32(code).ok_or("\\u escape outside the BMP")?;
                        bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                    _ => return Err(format!("bad escape at byte {}", self.i - 1)),
                },
                Some(c) => bytes.push(c),
            }
        }
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    #[test]
    fn fills_placeholders() {
        assert_eq!(fill("Saved to {folder}", &[("folder", "Movies")]), "Saved to Movies");
        assert_eq!(fill("{n} of {total}", &[("n", "2"), ("total", "5")]), "2 of 5");
    }

    #[test]
    fn plural_rules() {
        assert!(is_singular("en", 1) && !is_singular("en", 0));
        assert!(is_singular("fr", 0) && is_singular("pt-BR", 1) && !is_singular("fr", 2));
        assert!(!is_singular("ja", 1));
    }

    #[test]
    fn parses_escapes() {
        let c = parse_catalog(r#"{"a \"b\"": "c\nd", "é": "é"}"#).unwrap();
        assert_eq!(c, vec![("a \"b\"".into(), "c\nd".into()), ("é".into(), "é".into())]);
    }

    /// The repository root: the first folder above this crate that has `locales/`.
    fn root() -> PathBuf {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        while !dir.join("locales").is_dir() {
            assert!(dir.pop(), "no locales folder above the crate");
        }
        dir
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().is_some_and(|n| n != "target") {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Read a Rust string literal starting at `s[i] == '"'`; returns it and the index after it.
    fn literal(s: &[u8], mut i: usize) -> Option<(String, usize)> {
        if s.get(i) != Some(&b'"') {
            return None;
        }
        i += 1;
        let mut out = Vec::new();
        while i < s.len() {
            match s[i] {
                b'"' => return Some((String::from_utf8(out).ok()?, i + 1)),
                b'\\' => {
                    i += 1;
                    match s.get(i)? {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'\n' => {
                            // A line continuation: skip the newline and the indentation after it.
                            while matches!(s.get(i + 1), Some(b' ' | b'\t' | b'\n')) {
                                i += 1;
                            }
                        }
                        &c => out.push(c),
                    }
                }
                c => out.push(c),
            }
            i += 1;
        }
        None
    }

    fn skip_ws(s: &[u8], mut i: usize) -> usize {
        while matches!(s.get(i), Some(b' ' | b'\n' | b'\t' | b'\r')) {
            i += 1;
        }
        i
    }

    /// Every key passed to `tr`, `trf` or `trn` as a literal, with where it's used.
    fn source_keys() -> BTreeMap<String, String> {
        let root = root();
        let mut files = Vec::new();
        for dir in ["src", "windows/src"] {
            if root.join(dir).is_dir() {
                rust_files(&root.join(dir), &mut files);
            }
        }
        // The translation modules only mention the calls in comments and in this scanner.
        files.retain(|f| !f.file_name().is_some_and(|n| n == "i18n.rs" || n == "i18n_text.rs"));
        let mut keys = BTreeMap::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let s = text.as_bytes();
            for call in ["tr(", "trf(", "trn("] {
                let mut from = 0;
                while let Some(pos) = text[from..].find(call) {
                    let at = from + pos;
                    from = at + call.len();
                    // Skip longer names that end in the same letters, such as `attr(`.
                    if at > 0 && (s[at - 1].is_ascii_alphanumeric() || s[at - 1] == b'_') {
                        continue;
                    }
                    let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
                    if text[line_start..at].trim_start().starts_with("//") {
                        continue;
                    }
                    let mut i = skip_ws(s, from);
                    if call == "trn(" {
                        // Skip the count argument up to the first top-level comma.
                        let mut depth = 0;
                        while i < s.len() && !(depth == 0 && s[i] == b',') {
                            match s[i] {
                                b'(' | b'[' => depth += 1,
                                b')' | b']' => depth -= 1,
                                _ => {}
                            }
                            i += 1;
                        }
                        i = skip_ws(s, i + 1);
                    }
                    let where_ = file.strip_prefix(&root).unwrap().display().to_string();
                    let Some((key, next)) = literal(s, i) else { continue };
                    keys.insert(key, where_.clone());
                    if call == "trn(" {
                        let j = skip_ws(s, next);
                        if s.get(j) == Some(&b',')
                            && let Some((other, _)) = literal(s, skip_ws(s, j + 1))
                        {
                            keys.insert(other, where_);
                        }
                    }
                }
            }
        }
        // The permission prompts' text comes from Info.plist; bundle.sh translates it too.
        if let Ok(plist) = std::fs::read_to_string(root.join("resources/Info.plist")) {
            for chunk in plist.split("<key>").skip(1) {
                let Some((name, rest)) = chunk.split_once("</key>") else { continue };
                if !name.ends_with("UsageDescription") {
                    continue;
                }
                if let Some(value) = rest.split_once("<string>").and_then(|(_, v)| v.split_once("</string>")) {
                    keys.insert(value.0.to_string(), "resources/Info.plist".into());
                }
            }
        }
        keys
    }

    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            out.insert(rest[open + 1..open + close].to_string());
            rest = &rest[open + close + 1..];
        }
        out
    }

    /// Every catalog has every key, no stale keys, and the same placeholders as the English.
    #[test]
    fn catalogs_match_the_source() {
        let keys = source_keys();
        assert!(!keys.is_empty(), "no tr() calls found");
        let mut problems = Vec::new();
        for lang in LANGUAGES {
            let path = root().join(format!("locales/{lang}.json"));
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {}", path.display()));
            let catalog: BTreeMap<String, String> = parse_catalog(&text)
                .unwrap_or_else(|e| panic!("{lang}.json: {e}"))
                .into_iter()
                .collect();
            for (key, file) in &keys {
                match catalog.get(key) {
                    None => problems.push(format!("{lang}: missing {key:?} ({file})")),
                    Some(t) if t.trim().is_empty() => problems.push(format!("{lang}: empty {key:?}")),
                    Some(t) if placeholders(t) != placeholders(key) => {
                        problems.push(format!("{lang}: placeholders differ in {key:?} -> {t:?}"))
                    }
                    _ => {}
                }
            }
            for key in catalog.keys().filter(|k| !keys.contains_key(*k)) {
                problems.push(format!("{lang}: unused {key:?}"));
            }
        }
        assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
    }
}
