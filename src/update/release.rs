//! The answer from GitHub's `releases/latest`: the version, the notes and the
//! download, picked out of the JSON.

use serde_json::Value;

use super::{Error, Version};

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    /// Release notes, in Markdown (see `notes::parse`).
    pub notes: String,
    /// The release's page on GitHub, opened when the app can't replace itself.
    pub page: String,
    /// The zip to download.
    pub url: String,
    /// The zip's checksum as GitHub lists it ("sha256:…"). None when GitHub has none.
    pub digest: Option<String>,
    /// The zip's size in bytes, as GitHub reports it.
    pub size: u64,
}

/// Read a `releases/latest` answer. `Ok(None)` when the release doesn't count as
/// an update: a draft or a pre-release. `allow_file` lets a test feed point at a
/// local zip; GitHub's own downloads are always https.
pub fn parse(json: &[u8], asset: &str, repo: &str, allow_file: bool) -> Result<Option<Release>, Error> {
    let v: Value = serde_json::from_slice(json).map_err(|_| Error::Release("GitHub's answer wasn't a release.".into()))?;
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let flag = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    let Some(tag) = text("tag_name") else {
        return Err(Error::Release("GitHub's answer wasn't a release.".into()));
    };
    if flag("draft") || flag("prerelease") {
        return Ok(None);
    }
    let version = Version::parse(&tag)
        .ok_or_else(|| Error::Release(format!("The latest release's tag, {tag}, isn't a version number like v1.2.3.")))?;
    let (url, digest, size) = pick_asset(&v, asset)
        .ok_or_else(|| Error::Release(format!("The latest release has no {asset} to download.")))?;
    if !(url.starts_with("https://") || allow_file && url.starts_with("file://")) {
        return Err(Error::Release(format!("The latest release's {asset} isn't served over https.")));
    }
    Ok(Some(Release {
        version,
        notes: text("body").unwrap_or_default(),
        page: text("html_url").unwrap_or_else(|| format!("https://github.com/{repo}/releases/latest")),
        url,
        digest,
        size,
    }))
}

/// The asset with this exact name, if it finished uploading: its download URL,
/// digest and size.
fn pick_asset(release: &Value, name: &str) -> Option<(String, Option<String>, u64)> {
    let assets = release.get("assets")?.as_array()?;
    let a = assets.iter().find(|a| {
        a.get("name").and_then(Value::as_str) == Some(name)
            && a.get("state").and_then(Value::as_str).is_none_or(|s| s == "uploaded")
    })?;
    let url = a.get("browser_download_url")?.as_str()?.to_string();
    let digest = a.get("digest").and_then(Value::as_str).filter(|d| !d.is_empty()).map(str::to_string);
    let size = a.get("size").and_then(Value::as_u64).unwrap_or(0);
    Some((url, digest, size))
}

/// "sha256:<64 hex digits>" as raw bytes. None for any other algorithm or shape.
pub fn parse_digest(digest: &str) -> Option<[u8; 32]> {
    let (algorithm, hex) = digest.trim().split_once(':')?;
    if !algorithm.eq_ignore_ascii_case("sha256") || hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GitHub's answer for Tamka 0.1.1, shortened (one asset, short notes).
    const FIXTURE: &str = include_str!("fixtures/release.json");

    #[test]
    fn reads_a_release() {
        let r = parse(FIXTURE.as_bytes(), "Tamka-macOS-arm64.zip", "piedol/tamka", false).unwrap().unwrap();
        assert_eq!(r.version, Version::parse("0.1.1").unwrap());
        assert_eq!(r.page, "https://github.com/piedol/tamka/releases/tag/v0.1.1");
        assert_eq!(r.url, "https://github.com/piedol/tamka/releases/download/v0.1.1/Tamka-macOS-arm64.zip");
        assert_eq!(r.digest.as_deref(), Some("sha256:1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363"));
        assert_eq!(r.size, 10_222_162);
        assert!(r.notes.starts_with("Tamka 0.1.1 adds a microphone choice"));
    }

    #[test]
    fn picks_the_named_asset_only() {
        let json = r#"{"tag_name": "v2.0.0", "assets": [
            {"name": "Tamka-macOS-arm64.zip.sig", "browser_download_url": "https://x/sig", "state": "uploaded"},
            {"name": "Tamka-macOS-arm64.zip", "browser_download_url": "https://x/half", "state": "starter"},
            {"name": "Tamka-macOS-arm64.zip", "browser_download_url": "https://x/zip", "state": "uploaded", "size": 5}
        ]}"#;
        let r = parse(json.as_bytes(), "Tamka-macOS-arm64.zip", "piedol/tamka", false).unwrap().unwrap();
        assert_eq!(r.url, "https://x/zip");
        assert_eq!(r.digest, None);
        assert_eq!(r.page, "https://github.com/piedol/tamka/releases/latest");
        let none = r#"{"tag_name": "v2.0.0", "assets": [{"name": "Other.zip", "browser_download_url": "https://x"}]}"#;
        assert!(matches!(parse(none.as_bytes(), "Tamka-macOS-arm64.zip", "r", false), Err(Error::Release(_))));
    }

    #[test]
    fn drafts_and_prereleases_never_count() {
        for flag in ["draft", "prerelease"] {
            let json = format!(r#"{{"tag_name": "v9.0.0", "{flag}": true, "assets": []}}"#);
            assert!(parse(json.as_bytes(), "a.zip", "r", false).unwrap().is_none(), "{flag}");
        }
    }

    #[test]
    fn refuses_odd_answers() {
        let bad_tag = r#"{"tag_name": "v1.0-beta", "assets": []}"#;
        assert!(matches!(parse(bad_tag.as_bytes(), "a.zip", "r", false), Err(Error::Release(_))));
        assert!(parse(br#"{"message": "Not Found"}"#, "a.zip", "r", false).is_err());
        assert!(parse(b"<html>", "a.zip", "r", false).is_err());
        let http = r#"{"tag_name": "v1.0", "assets": [{"name": "a.zip", "browser_download_url": "http://x/a.zip"}]}"#;
        assert!(parse(http.as_bytes(), "a.zip", "r", false).is_err());
        let file = r#"{"tag_name": "v1.0", "assets": [{"name": "a.zip", "browser_download_url": "file:///tmp/a.zip"}]}"#;
        assert!(parse(file.as_bytes(), "a.zip", "r", false).is_err());
        assert!(parse(file.as_bytes(), "a.zip", "r", true).unwrap().is_some());
    }

    #[test]
    fn digests_parse() {
        let d = parse_digest("sha256:1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363").unwrap();
        assert_eq!(d[0], 0x1b);
        assert_eq!(d[31], 0x63);
        assert_eq!(parse_digest("SHA256:1BB88D2E8CDABF8D0296D07A3A947C9F4389DC7A214A012534F11F00A5509363"), Some(d));
        for bad in [
            "",
            "1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363",
            "sha1:1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363",
            "sha256:1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a55093",
            "sha256:zzb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363",
            "sha256:1bb88d2e8cdabf8d0296d07a3a947c9f4389dc7a214a012534f11f00a5509363ff",
        ] {
            assert!(parse_digest(bad).is_none(), "{bad}");
        }
    }
}
