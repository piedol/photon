//! Version numbers as release tags and CFBundleShortVersionString write them:
//! "v0.1.2", "0.1.2", "1.4".

use std::cmp::Ordering;
use std::fmt;

/// Numeric parts, compared one by one. Missing parts count as 0, so 1.4 equals 1.4.0.
#[derive(Clone, Debug)]
pub struct Version(Vec<u64>);

impl Version {
    /// "v1.2.3" or "1.2.3". None for anything else, including pre-release
    /// suffixes such as "1.2.3-beta", which never count as an update.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let parts: Option<Vec<u64>> = s
            .split('.')
            .map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None })
            .collect();
        parts.filter(|p| (1..=4).contains(&p.len())).map(Version)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.0.len().max(other.0.len());
        (0..n)
            .map(|i| self.0.get(i).unwrap_or(&0).cmp(other.0.get(i).unwrap_or(&0)))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn tags_and_bundle_versions_parse() {
        assert_eq!(v("v0.1.1"), v("0.1.1"));
        assert_eq!(v(" V2.0 ").to_string(), "2.0");
        assert_eq!(v("1.4"), v("1.4.0"));
        for bad in ["", "v", "1.2.3-beta", "1..2", "1.2.", "one", "1.2.3.4.5", "-1.0"] {
            assert!(Version::parse(bad).is_none(), "{bad} parsed");
        }
    }

    #[test]
    fn compares_numerically() {
        assert!(v("0.1.10") > v("0.1.9"));
        assert!(v("0.2") > v("0.1.99"));
        assert!(v("1.0.0") > v("0.9.9"));
        assert!(v("0.1.2") > v("0.1.1"));
        assert!(v("v0.1.1") > v("0.1.0"));
        assert!(v("0.1.1") <= v("0.1.1.0"));
        assert!(v("10.0") > v("9.10"));
    }
}
