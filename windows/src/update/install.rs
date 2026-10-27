//! Swapping the exe and starting the new one. Windows won't overwrite or delete a running
//! exe but lets it be renamed, so the running `Photon.exe` becomes `Photon.exe.old`, the new
//! one takes its name, and the new process removes the old file once this one has quit.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

/// Whether this user can create files in `folder`, which renaming the exe needs.
pub fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".photon-update-{}.tmp", std::process::id()));
    let ok = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// `Photon.exe` → `Photon.exe.old`.
pub fn old_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Rename `running` to its `.old` name and copy `new` to its path. Returns the old file's
/// path. If the new one can't be put in place, the old one is renamed back.
pub fn replace(running: &Path, new: &Path) -> Result<PathBuf, String> {
    let old = old_path(running);
    // Left by an earlier update whose old process was still exiting.
    let _ = std::fs::remove_file(&old);
    std::fs::rename(running, &old).map_err(|e| format!("The running app couldn't be renamed: {e}."))?;
    if let Err(e) = std::fs::copy(new, running) {
        let _ = std::fs::remove_file(running);
        let _ = std::fs::rename(&old, running);
        return Err(format!("The new app couldn't be put in place: {e}."));
    }
    Ok(old)
}

/// Start `exe` with `--updated <this pid>`, without the test overrides in `clear`.
pub fn start(exe: &Path, clear: &[String]) -> std::io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.arg("--updated").arg(std::process::id().to_string());
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    for name in clear {
        cmd.env_remove(name);
    }
    cmd.spawn().map(|_| ())
}

/// Remove `<exe>.old`, retrying while the process that ran it finishes quitting.
pub fn remove_old(exe: &Path, log: fn(&str)) {
    let old = old_path(exe);
    for _ in 0..50 {
        match std::fs::remove_file(&old) {
            Ok(()) => return log(&format!("update: removed {}", old.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    log(&format!("update: couldn't remove {}", old.display()));
}

/// Wait up to `limit` for process `pid` to exit (at once if it already has).
pub fn wait_for_exit(pid: u32, limit: Duration) {
    // SAFETY: a synchronize-only handle, closed after the wait.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return };
        WaitForSingleObject(h, limit.as_millis().min(u32::MAX as u128) as u32);
        let _ = CloseHandle(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photon-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn old_name_keeps_the_exe_name() {
        assert_eq!(old_path(Path::new("C:\\Tools\\Photon.exe")), PathBuf::from("C:\\Tools\\Photon.exe.old"));
    }

    #[test]
    fn replace_swaps_and_keeps_the_old_file() {
        let dir = scratch("swap");
        let (running, new) = (dir.join("Photon.exe"), dir.join("new.exe"));
        std::fs::write(&running, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        std::fs::write(old_path(&running), b"older").unwrap();
        assert!(writable(&dir));
        let old = replace(&running, &new).unwrap();
        assert_eq!(std::fs::read(&running).unwrap(), b"new");
        assert_eq!(std::fs::read(&old).unwrap(), b"old");
        remove_old(&running, |_| {});
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_copy_puts_the_old_exe_back() {
        let dir = scratch("undo");
        let running = dir.join("Photon.exe");
        std::fs::write(&running, b"old").unwrap();
        assert!(replace(&running, &dir.join("missing.exe")).is_err());
        assert_eq!(std::fs::read(&running).unwrap(), b"old");
        assert!(!old_path(&running).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_running_exe_can_be_swapped() {
        // This test binary is running: rename a copy of a running exe the same way.
        let dir = scratch("running");
        let exe = dir.join("ping.exe");
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32\\PING.EXE");
        std::fs::copy(&system, &exe).unwrap();
        let mut child = Command::new(&exe).args(["-n", "5", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        std::fs::write(dir.join("new.exe"), b"new").unwrap();
        let old = replace(&exe, &dir.join("new.exe")).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        // The old file stays locked until the process exits.
        assert!(std::fs::remove_file(&old).is_err());
        let _ = child.kill();
        let _ = child.wait();
        remove_old(&exe, |_| {});
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
