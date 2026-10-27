//! Cloud-sync folders on Windows. Like the Mac app, "sync" means writing into a
//! folder that the provider's own app already uploads; Photon does no networking.

use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, REG_VALUE_TYPE, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::UI::Shell::{FOLDERID_Pictures, FOLDERID_Videos, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{GUID, PWSTR};

use crate::util::{self, W};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OneDrive,
    GoogleDrive,
    Dropbox,
    Box,
    ICloudDrive,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::OneDrive => "OneDrive",
            Provider::GoogleDrive => "Google Drive",
            Provider::Dropbox => "Dropbox",
            Provider::Box => "Box",
            Provider::ICloudDrive => "iCloud Drive",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudFolder {
    pub provider: Provider,
    pub root: PathBuf,
    pub label: String,
    pub default_target: PathBuf,
}

/// The user's real Pictures or Videos folder (it may be redirected into OneDrive).
pub fn user_folder(which: &str) -> Option<PathBuf> {
    let id: &GUID = match which {
        "Pictures" => &FOLDERID_Pictures,
        "Videos" => &FOLDERID_Videos,
        _ => return None,
    };
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

fn reg_string(key: HKEY, sub: &str, value: &str) -> Option<String> {
    unsafe {
        let mut h = HKEY::default();
        let sub_w = W::new(sub);
        if RegOpenKeyExW(key, sub_w.p(), Some(0), KEY_READ, &mut h).is_err() {
            return None;
        }
        let name = W::new(value);
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let ok = RegQueryValueExW(h, name.p(), None, Some(&mut kind), Some(buf.as_mut_ptr() as *mut u8), Some(&mut len)).is_ok();
        let _ = RegCloseKey(h);
        ok.then(|| util::from_wide(&buf)).filter(|s| !s.is_empty())
    }
}

fn reg_subkeys(key: HKEY, sub: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut h = HKEY::default();
        let sub_w = W::new(sub);
        if RegOpenKeyExW(key, sub_w.p(), Some(0), KEY_READ, &mut h).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = vec![0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(h, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(util::from_wide(&name));
        }
        let _ = RegCloseKey(h);
    }
    out
}

fn volume_label(root: &str) -> Option<String> {
    let mut label = vec![0u16; 261];
    let r = W::new(root);
    unsafe { GetVolumeInformationW(r.p(), Some(&mut label), None, None, None, None).ok()? };
    Some(util::from_wide(&label))
}

fn push(out: &mut Vec<CloudFolder>, provider: Provider, account: Option<String>, root: PathBuf, target: PathBuf) {
    if !root.is_dir() || out.iter().any(|f| f.root == root) {
        return;
    }
    let label = match account {
        Some(a) if !a.is_empty() => format!("{} · {a}", provider.name()),
        _ => provider.name().to_string(),
    };
    out.push(CloudFolder { provider, root, label, default_target: target });
}

/// Every cloud folder we can find, sorted by label.
pub fn discover() -> Vec<CloudFolder> {
    let mut out = Vec::new();
    let home = util::home();

    // OneDrive: one registry entry per signed-in account.
    for acct in reg_subkeys(HKEY_CURRENT_USER, "Software\\Microsoft\\OneDrive\\Accounts") {
        let sub = format!("Software\\Microsoft\\OneDrive\\Accounts\\{acct}");
        if let Some(folder) = reg_string(HKEY_CURRENT_USER, &sub, "UserFolder") {
            let email = reg_string(HKEY_CURRENT_USER, &sub, "UserEmail");
            let root = PathBuf::from(folder);
            let label = email.or_else(|| Some(if acct == "Personal" { crate::i18n::tr("Personal") } else { acct.clone() }));
            push(&mut out, Provider::OneDrive, label, root.clone(), root.join("Photon"));
        }
    }
    for var in ["OneDriveConsumer", "OneDriveCommercial", "OneDrive"] {
        if let Some(p) = std::env::var_os(var) {
            let root = PathBuf::from(p);
            push(&mut out, Provider::OneDrive, None, root.clone(), root.join("Photon"));
        }
    }

    // Google Drive for desktop mounts a drive labelled "Google Drive" (or mirrors into the profile).
    let drives = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if drives & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        if volume_label(&root).is_some_and(|l| l.eq_ignore_ascii_case("Google Drive")) {
            let my = PathBuf::from(&root).join("My Drive");
            let base = if my.is_dir() { my } else { PathBuf::from(&root) };
            push(&mut out, Provider::GoogleDrive, None, PathBuf::from(&root), base.join("Photon"));
        }
    }
    for p in [home.join("My Drive"), home.join("Google Drive")] {
        push(&mut out, Provider::GoogleDrive, None, p.clone(), p.join("Photon"));
    }

    // Dropbox writes its folder path to info.json.
    for base in [std::env::var_os("APPDATA"), std::env::var_os("LOCALAPPDATA")].into_iter().flatten() {
        if let Ok(json) = std::fs::read_to_string(PathBuf::from(base).join("Dropbox\\info.json")) {
            for path in json_paths(&json) {
                let root = PathBuf::from(path);
                push(&mut out, Provider::Dropbox, None, root.clone(), root.join("Photon"));
            }
        }
    }
    let dbx = home.join("Dropbox");
    push(&mut out, Provider::Dropbox, None, dbx.clone(), dbx.join("Photon"));

    let bx = home.join("Box");
    push(&mut out, Provider::Box, None, bx.clone(), bx.join("Photon"));
    let icloud = home.join("iCloudDrive");
    push(&mut out, Provider::ICloudDrive, None, icloud.clone(), icloud.join("Photon"));

    out.sort_by_key(|f| f.label.to_lowercase());
    out
}

/// Every `"path": "…"` value in Dropbox's info.json (one per account).
fn json_paths(json: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(i) = rest.find("\"path\"") {
        rest = &rest[i + 6..];
        let Some(q1) = rest.find('"') else { break };
        let after = &rest[q1 + 1..];
        let mut val = String::new();
        let mut chars = after.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        val.push(n);
                    }
                }
                '"' => break,
                _ => val.push(c),
            }
        }
        out.push(val);
        rest = &after[after.find('"').unwrap_or(0)..];
    }
    out
}

/// Short display form: "OneDrive · me@x.com › Photon" or "~\Videos\Photon".
pub fn display_path(path: &Path, folders: &[CloudFolder]) -> String {
    let best = folders.iter().filter(|f| path.starts_with(&f.root)).max_by_key(|f| f.root.as_os_str().len());
    match best {
        Some(f) => {
            let rest: Vec<String> = path
                .strip_prefix(&f.root)
                .map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            if rest.is_empty() { f.label.clone() } else { format!("{} › {}", f.label, rest.join(" › ")) }
        }
        None => util::tilde(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropbox_info_json() {
        let j = r#"{"personal": {"path": "C:\\Users\\me\\Dropbox", "host": 1}, "business": {"path": "D:\\Work Dropbox"}}"#;
        assert_eq!(json_paths(j), vec!["C:\\Users\\me\\Dropbox".to_string(), "D:\\Work Dropbox".to_string()]);
    }
}
