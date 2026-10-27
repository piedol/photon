//! The checks a download passes before it may replace the exe: its SHA-256 (BCrypt) against
//! the release's digest, unpacking with Windows' tar.exe, and the new exe's version resource.

use std::ffi::c_void;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows::Win32::Security::Cryptography::{
    BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::util::W;

// ---------------------------------------------------------------- SHA-256

/// A BCrypt hash, destroyed when dropped.
struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        // SAFETY: a handle BCryptCreateHash returned, destroyed once.
        unsafe {
            let _ = BCryptDestroyHash(self.0);
        }
    }
}

pub fn sha256(mut input: impl Read) -> std::io::Result<[u8; 32]> {
    let fail = |what: &str| std::io::Error::other(format!("BCrypt couldn't {what}"));
    let mut handle = BCRYPT_HASH_HANDLE::default();
    // SAFETY: the SHA-256 pseudo-handle needs no provider or hash object buffer (Windows 10+).
    if unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut handle, None, None, 0) }.is_err() {
        return Err(fail("start a hash"));
    }
    let hash = Hash(handle);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        // SAFETY: hashes the `n` bytes just read.
        if unsafe { BCryptHashData(hash.0, &buf[..n], 0) }.is_err() {
            return Err(fail("hash the download"));
        }
    }
    let mut out = [0u8; 32];
    // SAFETY: SHA-256 output is 32 bytes.
    if unsafe { BCryptFinishHash(hash.0, &mut out, 0) }.is_err() {
        return Err(fail("finish the hash"));
    }
    Ok(out)
}

pub fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    sha256(std::fs::File::open(path)?)
}

// ---------------------------------------------------------------- unpacking

/// Windows' own tar.exe (bsdtar), which reads zip files and refuses absolute paths and `..`.
fn tar() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    root.join("System32\\tar.exe")
}

/// Unpack `zip` into `into`, which must be new.
pub fn unzip(zip: &Path, into: &Path) -> Result<(), String> {
    std::fs::create_dir_all(into).map_err(|e| format!("couldn't make {}: {e}", into.display()))?;
    let out = Command::new(tar())
        .arg("-x")
        .arg("-f")
        .arg(zip)
        .arg("-C")
        .arg(into)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
        .map_err(|e| format!("tar.exe didn't run: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("tar.exe failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

// ---------------------------------------------------------------- version resource

/// The exe's ProductName and product version ("0.2.1"), from its version resource.
pub fn exe_info(exe: &Path) -> Option<(String, String)> {
    let path = W::new(&exe.to_string_lossy());
    // SAFETY: the buffer is sized by GetFileVersionInfoSizeW, and VerQueryValueW returns
    // pointers into it, read while it lives and within the lengths it reports.
    unsafe {
        let size = GetFileVersionInfoSizeW(path.p(), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(path.p(), None, size, data.as_mut_ptr().cast()).ok()?;
        let block = data.as_ptr() as *const c_void;
        let query = |sub: &str| -> Option<(*const c_void, u32)> {
            let sub = W::new(sub);
            let mut ptr: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            VerQueryValueW(block, sub.p(), &mut ptr, &mut len).as_bool().then_some((ptr as *const c_void, len))
        };
        let (fixed, len) = query("\\")?;
        if (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let fixed = &*(fixed as *const VS_FIXEDFILEINFO);
        let (ms, ls) = (fixed.dwProductVersionMS, fixed.dwProductVersionLS);
        let mut parts = vec![ms >> 16, ms & 0xffff, ls >> 16];
        if ls & 0xffff != 0 {
            parts.push(ls & 0xffff);
        }
        let version = parts.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
        // The first language and code page the resource lists names its strings.
        let (langs, len) = query("\\VarFileInfo\\Translation")?;
        if len < 4 {
            return None;
        }
        let pair = std::slice::from_raw_parts(langs as *const u16, 2);
        let (name, len) = query(&format!("\\StringFileInfo\\{:04x}{:04x}\\ProductName", pair[0], pair[1]))?;
        let name = std::slice::from_raw_parts(name as *const u16, len as usize);
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        Some((String::from_utf16_lossy(&name[..end]), version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: [u8; 32]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha256_matches_known_values() {
        assert_eq!(hex(sha256(&b""[..]).unwrap()), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(sha256(&b"abc"[..]).unwrap()), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // Longer than one read buffer.
        let big = vec![b'a'; 3_000_000];
        assert_eq!(hex(sha256(&big[..]).unwrap()).len(), 64);
        let million = vec![b'a'; 1_000_000];
        assert_eq!(hex(sha256(&million[..]).unwrap()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn reads_a_version_resource() {
        let notepad = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32\\notepad.exe");
        let (name, version) = exe_info(&notepad).expect("notepad has a version resource");
        assert!(!name.is_empty());
        assert!(super::super::Version::parse(&version).is_some(), "{version}");
        let text = std::env::temp_dir().join(format!("photon-update-test-{}.txt", std::process::id()));
        std::fs::write(&text, b"not an exe").unwrap();
        assert_eq!(exe_info(&text), None);
        let _ = std::fs::remove_file(&text);
    }
}
