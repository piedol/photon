//! Swapping the app bundle and reopening it: the running bundle goes to the
//! Trash, the new one takes its path, and a detached shell opens it once this
//! process has quit.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use objc2_foundation::{NSFileManager, NSString, NSURL};

fn url(path: &Path) -> objc2::rc::Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// Whether this user can move `bundle` out of its folder and put another in its place.
/// Moving a folder to another parent also needs write access to the folder itself.
pub fn writable(bundle: &Path) -> bool {
    let fm = NSFileManager::defaultManager();
    let ok = |p: &Path| fm.isWritableFileAtPath(&NSString::from_str(&p.to_string_lossy()));
    bundle.parent().is_some_and(ok) && ok(bundle)
}

/// Move `running` to the Trash and `new` to its path. Returns where the old bundle
/// went. If the new one can't be moved in, the old one is put back.
pub fn replace(running: &Path, new: &Path) -> Result<PathBuf, String> {
    let fm = NSFileManager::defaultManager();
    let mut trashed = None;
    fm.trashItemAtURL_resultingItemURL_error(&url(running), Some(&mut trashed))
        .map_err(|e| format!("The current app couldn't be moved to the Trash: {}", e.localizedDescription()))?;
    let trashed = trashed.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()));
    if let Err(e) = fm.moveItemAtURL_toURL_error(&url(new), &url(running)) {
        let why = e.localizedDescription().to_string();
        if let Some(t) = &trashed {
            let _ = fm.moveItemAtURL_toURL_error(&url(t), &url(running));
        }
        return Err(format!("The new app couldn't be moved into place: {why}"));
    }
    Ok(trashed.unwrap_or_default())
}

/// Start a shell, outside this process's group, that waits up to a minute for
/// this process to exit and then opens `app`. `-n` opens the new copy even if
/// another copy with the same bundle id is running somewhere else.
pub fn relaunch_after_exit(app: &Path) -> std::io::Result<()> {
    let script = r#"i=0
while kill -0 "$1" 2>/dev/null; do
  i=$((i + 1)); [ "$i" -gt 600 ] && exit 1
  sleep 0.1
done
exec /usr/bin/open -n "$2""#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("relaunch")
        .arg(std::process::id().to_string())
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_folders_are_not_writable() {
        let dir = std::env::temp_dir().join(format!("update-install-{}", std::process::id()));
        let app = dir.join("Fake.app");
        std::fs::create_dir_all(&app).unwrap();
        assert!(writable(&app));
        let mode = |p: &Path, m: u32| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        };
        mode(&dir, 0o555);
        assert!(!writable(&app), "the folder holding the app is read-only");
        mode(&dir, 0o755);
        mode(&app, 0o555);
        assert!(!writable(&app), "the app bundle itself is read-only");
        mode(&app, 0o755);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
