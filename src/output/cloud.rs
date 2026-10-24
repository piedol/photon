//! Discovery of folders that cloud-storage apps already sync.
//!
//! Photon does no networking: "saving to the cloud" means saving into a
//! folder under `~/Library/CloudStorage` (OneDrive, Google Drive, Dropbox,
//! Box) or into iCloud Drive.

use std::path::{Path, PathBuf};

/// Folder Photon creates inside a provider root.
const TARGET_DIR: &str = "Photon";
/// Google Drive's personal-files subfolder.
const MY_DRIVE: &str = "My Drive";

/// A cloud-storage provider with a macOS sync app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OneDrive,
    GoogleDrive,
    Dropbox,
    Box,
    ICloudDrive,
}

impl Provider {
    /// Display name, e.g. "Google Drive".
    pub fn name(self) -> &'static str {
        match self {
            Self::OneDrive => "OneDrive",
            Self::GoogleDrive => "Google Drive",
            Self::Dropbox => "Dropbox",
            Self::Box => "Box",
            Self::ICloudDrive => "iCloud Drive",
        }
    }
}

/// A synced folder Photon can save into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudFolder {
    pub provider: Provider,
    /// Account part of the folder name, e.g. "Personal"; `None` for iCloud.
    pub account: Option<String>,
    /// The provider's root folder.
    pub root: PathBuf,
    /// UI label, e.g. "OneDrive · Personal".
    pub label: String,
    /// Suggested save folder: `root/Photon`, or `root/My Drive/Photon` for
    /// Google Drive when "My Drive" exists.
    pub default_target: PathBuf,
    /// The name ends with a " (…)" suffix, as left behind by a reinstall.
    pub stale: bool,
}

/// Finds synced folders for the current user (via `$HOME`).
pub fn discover() -> Vec<CloudFolder> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    discover_in(&home.join("Library/CloudStorage"), Some(&icloud))
}

/// Finds synced folders in a CloudStorage folder, plus iCloud Drive when
/// `icloud_docs` exists. Non-stale entries sort first, then by label.
pub fn discover_in(cloud_storage: &Path, icloud_docs: Option<&Path>) -> Vec<CloudFolder> {
    let mut folders = Vec::new();
    if let Ok(entries) = std::fs::read_dir(cloud_storage) {
        for entry in entries.flatten() {
            let root = entry.path();
            if !root.is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            if let Some(folder) = file_name.to_str().and_then(|name| parse_entry(name, root)) {
                folders.push(folder);
            }
        }
    }
    if let Some(docs) = icloud_docs.filter(|p| p.is_dir()) {
        folders.push(CloudFolder {
            provider: Provider::ICloudDrive,
            account: None,
            root: docs.to_path_buf(),
            label: Provider::ICloudDrive.name().to_owned(),
            default_target: docs.join(TARGET_DIR),
            stale: false,
        });
    }
    folders.sort_by(|a, b| {
        a.stale
            .cmp(&b.stale)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.label.cmp(&b.label))
    });
    folders
}

/// The folder with the longest root that contains `path` (or equals it).
pub fn folder_for_path(path: &Path, folders: &[CloudFolder]) -> Option<CloudFolder> {
    folders
        .iter()
        .filter(|f| path.starts_with(&f.root))
        .max_by_key(|f| f.root.components().count())
        .cloned()
}

/// Short display form of `path`: "OneDrive · Personal › Photon" inside a
/// cloud root, "~/Movies/Photon" inside `$HOME`, otherwise the full path.
pub fn display_path(path: &Path, folders: &[CloudFolder]) -> String {
    display_path_with_home(path, folders, home_dir().as_deref())
}

/// [`display_path`] with an explicit home folder.
pub fn display_path_with_home(path: &Path, folders: &[CloudFolder], home: Option<&Path>) -> String {
    if let Some(folder) = folder_for_path(path, folders) {
        let mut out = folder.label;
        if let Ok(rest) = path.strip_prefix(&folder.root) {
            for part in rest.components() {
                out.push_str(" › ");
                out.push_str(&part.as_os_str().to_string_lossy());
            }
        }
        return out;
    }
    // A home of "/" would turn every path into "~/…", so ignore it.
    if let Some(rest) = home
        .filter(|h| h.parent().is_some())
        .and_then(|h| path.strip_prefix(h).ok())
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Builds a folder from a CloudStorage entry name, or `None` if the name is
/// not a known provider.
fn parse_entry(name: &str, root: PathBuf) -> Option<CloudFolder> {
    let (provider, account, suffix) = parse_name(name)?;
    let mut label = provider.name().to_owned();
    if let Some(account) = &account {
        label.push_str(" · ");
        label.push_str(account);
    }
    label.push_str(suffix);

    let my_drive = root.join(MY_DRIVE);
    let default_target = if provider == Provider::GoogleDrive && my_drive.is_dir() {
        my_drive.join(TARGET_DIR)
    } else {
        root.join(TARGET_DIR)
    };
    Some(CloudFolder {
        provider,
        account,
        default_target,
        root,
        label,
        stale: has_stale_suffix(name),
    })
}

/// Splits `Provider[-Account][ (…)]` into provider, account and any
/// " (…)" suffix that follows a bare provider name.
fn parse_name(name: &str) -> Option<(Provider, Option<String>, &str)> {
    const PREFIXES: [(&str, Provider); 4] = [
        ("OneDrive", Provider::OneDrive),
        ("GoogleDrive", Provider::GoogleDrive),
        ("Dropbox", Provider::Dropbox),
        ("Box", Provider::Box),
    ];
    for (prefix, provider) in PREFIXES {
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            return Some((provider, None, ""));
        }
        if let Some(account) = rest.strip_prefix('-') {
            let account = (!account.is_empty()).then(|| account.to_owned());
            return Some((provider, account, ""));
        }
        if rest.starts_with(" (") {
            return Some((provider, None, rest));
        }
        // e.g. "Boxcryptor": a different app that happens to share a prefix.
    }
    None
}

/// True for names ending in " (…)" with something inside the parentheses.
fn has_stale_suffix(name: &str) -> bool {
    name.strip_suffix(')')
        .and_then(|s| s.rfind(" (").map(|i| &s[i + 2..]))
        .is_some_and(|inner| !inner.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let name = format!(
                "photon-cloud-{}-{nanos}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            );
            let dir = std::env::temp_dir().join(name);
            fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn mkdir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).expect("mkdir");
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const GMAIL: &str = "GoogleDrive-pat@example.com";
    const WORK: &str = "GoogleDrive-morgan@work.example";
    const STALE: &str = "OneDrive-Personal (11-09-2026 4:05 PM)";

    /// Mirrors the real Mac's CloudStorage plus some edge cases.
    fn fake_tree() -> (TempDir, PathBuf, PathBuf) {
        let t = TempDir::new();
        let cs = t.mkdir("CloudStorage");
        for d in [
            "OneDrive-Personal",
            STALE,
            WORK,
            "MacDroid-NintendoSwitch",
            "Dropbox",
            "Boxcryptor",
        ] {
            t.mkdir(&format!("CloudStorage/{d}"));
        }
        t.mkdir(&format!("CloudStorage/{GMAIL}/My Drive"));
        fs::write(cs.join("Box-Work"), b"not a dir").expect("write");
        let icloud = t.mkdir("Mobile Documents/com~apple~CloudDocs");
        (t, cs, icloud)
    }

    #[test]
    fn discovers_and_sorts() {
        let (_t, cs, icloud) = fake_tree();
        let found = discover_in(&cs, Some(&icloud));
        let labels: Vec<_> = found.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Dropbox",
                "Google Drive · morgan@work.example",
                "Google Drive · pat@example.com",
                "iCloud Drive",
                "OneDrive · Personal",
                "OneDrive · Personal (11-09-2026 4:05 PM)",
            ]
        );
        let stale: Vec<_> = found.iter().map(|f| f.stale).collect();
        assert_eq!(stale, [false, false, false, false, false, true]);
    }

    #[test]
    fn folder_fields() {
        let (_t, cs, icloud) = fake_tree();
        let found = discover_in(&cs, Some(&icloud));
        let by_label = |l: &str| found.iter().find(|f| f.label == l).expect("folder present");

        let gmail = by_label("Google Drive · pat@example.com");
        assert_eq!(gmail.provider, Provider::GoogleDrive);
        assert_eq!(gmail.account.as_deref(), Some("pat@example.com"));
        assert_eq!(gmail.root, cs.join(GMAIL));
        assert_eq!(gmail.default_target, cs.join(GMAIL).join("My Drive/Photon"));

        let work = by_label("Google Drive · morgan@work.example");
        assert_eq!(work.default_target, cs.join(WORK).join("Photon"));

        let od = by_label("OneDrive · Personal");
        assert_eq!(
            (od.provider, od.account.as_deref()),
            (Provider::OneDrive, Some("Personal"))
        );
        assert_eq!(od.default_target, cs.join("OneDrive-Personal/Photon"));

        let dropbox = by_label("Dropbox");
        assert_eq!(
            (dropbox.provider, dropbox.account.as_deref()),
            (Provider::Dropbox, None)
        );

        let ic = by_label("iCloud Drive");
        assert_eq!(
            (ic.provider, ic.account.as_deref()),
            (Provider::ICloudDrive, None)
        );
        assert_eq!(ic.default_target, icloud.join("Photon"));
    }

    #[test]
    fn missing_folders() {
        let t = TempDir::new();
        assert!(discover_in(&t.0.join("nope"), None).is_empty());
        assert!(discover_in(&t.0.join("nope"), Some(&t.0.join("also-nope"))).is_empty());
        let (_t, cs, _) = fake_tree();
        assert!(
            discover_in(&cs, None)
                .iter()
                .all(|f| f.provider != Provider::ICloudDrive)
        );
    }

    #[test]
    fn discover_does_not_panic() {
        let _ = discover();
    }

    #[test]
    fn parses_names() {
        let p = |n| parse_name(n).map(|(p, a, s)| (p, a, s.to_owned()));
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(
            p("Dropbox-Personal"),
            Some((Provider::Dropbox, some("Personal"), String::new()))
        );
        assert_eq!(
            p("Box-Acme"),
            Some((Provider::Box, some("Acme"), String::new()))
        );
        assert_eq!(p("Box"), Some((Provider::Box, None, String::new())));
        assert_eq!(
            p("Dropbox (2)"),
            Some((Provider::Dropbox, None, " (2)".to_owned()))
        );
        assert_eq!(
            p("OneDrive-"),
            Some((Provider::OneDrive, None, String::new()))
        );
        assert_eq!(p("Boxcryptor"), None);
        assert_eq!(p("MacDroid-NintendoSwitch"), None);
        assert_eq!(p("onedrive-Personal"), None);

        assert!(has_stale_suffix(STALE));
        assert!(has_stale_suffix("Dropbox (2)"));
        assert!(!has_stale_suffix("OneDrive-Personal"));
        assert!(!has_stale_suffix("OneDrive-Personal ()"));
        assert!(!has_stale_suffix("OneDrive-Personal(2)"));
    }

    fn folder(provider: Provider, root: &str, label: &str) -> CloudFolder {
        CloudFolder {
            provider,
            account: None,
            root: PathBuf::from(root),
            label: label.to_owned(),
            default_target: Path::new(root).join("Photon"),
            stale: false,
        }
    }

    #[test]
    fn longest_root_wins() {
        let folders = [
            folder(Provider::Box, "/c", "Outer"),
            folder(Provider::Box, "/c/inner", "Inner"),
        ];
        let label = |p: &str| folder_for_path(Path::new(p), &folders).map(|f| f.label);
        assert_eq!(label("/c/inner/x").as_deref(), Some("Inner"));
        assert_eq!(label("/c/inner").as_deref(), Some("Inner"));
        assert_eq!(label("/c/innerish").as_deref(), Some("Outer"));
        assert_eq!(label("/d"), None);
    }

    #[test]
    fn display_paths() {
        let (_t, cs, icloud) = fake_tree();
        let found = discover_in(&cs, Some(&icloud));
        let home = Path::new("/Users/me");
        let show = |p: &Path| display_path_with_home(p, &found, Some(home));

        assert_eq!(
            show(&cs.join("OneDrive-Personal/Photon")),
            "OneDrive · Personal › Photon"
        );
        assert_eq!(
            show(&cs.join(GMAIL).join("My Drive/Photon/Shots")),
            "Google Drive · pat@example.com › My Drive › Photon › Shots"
        );
        assert_eq!(show(&cs.join("Dropbox")), "Dropbox");
        assert_eq!(show(&icloud.join("Photon")), "iCloud Drive › Photon");
        assert_eq!(
            show(Path::new("/Users/me/Movies/Photon")),
            "~/Movies/Photon"
        );
        assert_eq!(show(Path::new("/Users/me")), "~");
        assert_eq!(
            show(Path::new("/Volumes/Ext/Photon")),
            "/Volumes/Ext/Photon"
        );
        assert_eq!(
            display_path_with_home(Path::new("/x/y"), &found, Some(Path::new("/"))),
            "/x/y"
        );
        assert_eq!(
            display_path_with_home(Path::new("/x/y"), &found, None),
            "/x/y"
        );
    }

    #[test]
    fn provider_names() {
        assert_eq!(Provider::GoogleDrive.name(), "Google Drive");
        assert_eq!(Provider::ICloudDrive.name(), "iCloud Drive");
    }
}
