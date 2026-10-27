//! Where finished files go. Today every destination is a folder (local or a
//! cloud-sync folder); a future upload destination can implement the same trait.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::naming::{self, Kind, LocalTime, NameContext, NamingRule};
use crate::i18n::{tr, trf};
use crate::util;

fn couldnt_create(dir: &Path, e: std::io::Error) -> String {
    trf("Couldn't create {folder}: {error}", &[("folder", &util::tilde(dir)), ("error", &e.to_string())])
}

pub trait Destination {
    /// Deliver `file`, returning where it ended up.
    fn deliver(&self, file: &Path, name: &str) -> Result<PathBuf, String>;
}

/// A folder destination.
pub struct Folder(pub PathBuf);

impl Destination for Folder {
    fn deliver(&self, file: &Path, name: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.0).map_err(|e| couldnt_create(&self.0, e))?;
        let target = self.0.join(name);
        move_or_copy(file, &target)?;
        Ok(target)
    }
}

/// The provider root a path lives under, when it is inside `~/Library/CloudStorage`.
pub fn cloud_root(path: &Path) -> Option<PathBuf> {
    let storage = util::home().join("Library/CloudStorage");
    let rest = path.strip_prefix(&storage).ok()?;
    let first = rest.components().next()?;
    Some(storage.join(first))
}

/// Whether a chosen folder can be used right now. Cloud folders need their
/// provider root to exist (we never create a fake provider folder).
pub fn is_usable(path: &Path) -> bool {
    if let Some(root) = cloud_root(path) {
        return root.is_dir();
    }
    // Local: usable if it exists or its nearest existing ancestor is a directory.
    path.ancestors().find(|a| a.exists()).is_some_and(|a| a.is_dir())
}

pub struct Resolved {
    pub dir: PathBuf,
    pub fell_back: bool,
}

/// Pick `preferred`, or `fallback` when the preferred folder is unavailable.
pub fn resolve(preferred: &Path, fallback: &Path) -> Resolved {
    if is_usable(preferred) {
        Resolved { dir: preferred.to_path_buf(), fell_back: false }
    } else {
        Resolved { dir: fallback.to_path_buf(), fell_back: true }
    }
}

pub struct NameParts {
    pub kind: Kind,
    pub app: Option<String>,
    pub size: Option<(u32, u32)>,
    pub display: Option<String>,
}

/// A unique file path in `dir` for the rule. Returns the path and the counter used.
pub fn unique_path(dir: &Path, rule: &NamingRule, counter: u32, parts: &NameParts, ext: &str) -> (PathBuf, u32) {
    let ctx = NameContext {
        kind: parts.kind,
        when: LocalTime::now(),
        app: parts.app.clone(),
        size: parts.size,
        display: parts.display.clone(),
        counter,
    };
    naming::next_available(dir, rule, &ctx, ext)
}

pub fn file_name_preview(rule: &NamingRule, counter: u32, parts: &NameParts, ext: &str) -> String {
    let ctx = NameContext {
        kind: parts.kind,
        when: LocalTime::now(),
        app: parts.app.clone(),
        size: parts.size,
        display: parts.display.clone(),
        counter,
    };
    format!("{}.{ext}", naming::render_stem(rule, &ctx))
}

/// Write bytes atomically (temp file in the same folder, then rename).
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = target.parent().ok_or_else(|| tr("bad path"))?;
    fs::create_dir_all(dir).map_err(|e| couldnt_create(dir, e))?;
    let tmp = dir.join(format!(".photon-{}.tmp", std::process::id()));
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    f.sync_all().ok();
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Rename, or copy then delete when crossing volumes.
pub fn move_or_copy(from: &Path, to: &Path) -> Result<(), String> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)
        .map_err(|e| trf("Couldn't save to {path}: {error}", &[("path", &util::tilde(to)), ("error", &e.to_string())]))?;
    let _ = fs::remove_file(from);
    Ok(())
}

/// Folder for in-progress recordings (cleaned at launch).
pub fn scratch_dir() -> PathBuf {
    let d = std::env::temp_dir().join("photon-recordings");
    let _ = fs::create_dir_all(&d);
    d
}

/// Remove leftovers from a crash or force-quit.
pub fn clean_scratch() {
    if let Ok(entries) = fs::read_dir(scratch_dir()) {
        for e in entries.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_are_usable_when_ancestor_exists() {
        let p = std::env::temp_dir().join("photon-test-does-not-exist/sub");
        assert!(is_usable(&p));
    }

    #[test]
    fn missing_cloud_provider_is_unusable() {
        let p = util::home().join("Library/CloudStorage/OneDrive-NoSuchAccount-xyz/Photon");
        assert!(!is_usable(&p));
        let r = resolve(&p, Path::new("/tmp"));
        assert!(r.fell_back);
        assert_eq!(r.dir, PathBuf::from("/tmp"));
    }

    #[test]
    fn cloud_root_extraction() {
        let p = util::home().join("Library/CloudStorage/GoogleDrive-a@b.com/My Drive/Photon");
        assert_eq!(cloud_root(&p), Some(util::home().join("Library/CloudStorage/GoogleDrive-a@b.com")));
        assert_eq!(cloud_root(Path::new("/tmp/x")), None);
    }

    #[test]
    fn atomic_write_and_move() {
        let dir = std::env::temp_dir().join(format!("photon-dest-test-{}", std::process::id()));
        let a = dir.join("a.bin");
        write_atomic(&a, b"hello").unwrap();
        assert_eq!(fs::read(&a).unwrap(), b"hello");
        let out = Folder(dir.join("out")).deliver(&a, "b.bin").unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"hello");
        assert!(!a.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
