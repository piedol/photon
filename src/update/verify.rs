//! The checks a download passes before it may replace the app: its SHA-256
//! against the release's digest, unpacking with ditto, and the code signature
//! (developer, bundle id, notarization) through the Security framework.

use std::ffi::c_void;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSError, NSString, NSURL};

// ---------------------------------------------------------------- SHA-256

/// CommonCrypto's CC_SHA256_CTX.
#[repr(C)]
struct Sha256Ctx {
    count: [u32; 2],
    hash: [u32; 8],
    wbuf: [u32; 16],
}

unsafe extern "C" {
    // libcommonCrypto, part of libSystem.
    fn CC_SHA256_Init(c: *mut Sha256Ctx) -> i32;
    fn CC_SHA256_Update(c: *mut Sha256Ctx, data: *const c_void, len: u32) -> i32;
    fn CC_SHA256_Final(md: *mut u8, c: *mut Sha256Ctx) -> i32;
}

pub fn sha256(mut input: impl Read) -> std::io::Result<[u8; 32]> {
    let mut ctx = Sha256Ctx { count: [0; 2], hash: [0; 8], wbuf: [0; 16] };
    let mut buf = vec![0u8; 1 << 20];
    // SAFETY: ctx is a CC_SHA256_CTX; each update reads `n` initialized bytes of buf.
    unsafe { CC_SHA256_Init(&mut ctx) };
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        unsafe { CC_SHA256_Update(&mut ctx, buf.as_ptr().cast(), n as u32) };
    }
    let mut out = [0u8; 32];
    unsafe { CC_SHA256_Final(out.as_mut_ptr(), &mut ctx) };
    Ok(out)
}

pub fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    sha256(std::fs::File::open(path)?)
}

// ---------------------------------------------------------------- unpacking

/// Unpack a zip made by `ditto -c -k --keepParent` into `into`, which must be new and empty.
pub fn unzip(zip: &Path, into: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(into)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("ditto didn't run: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("ditto failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

// ---------------------------------------------------------------- code signature

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecStaticCodeCreateWithPath(path: *const c_void, flags: u32, code: *mut *const c_void) -> i32;
    fn SecRequirementCreateWithString(text: *const c_void, flags: u32, requirement: *mut *const c_void) -> i32;
    fn SecStaticCodeCheckValidityWithErrors(
        code: *const c_void,
        flags: u32,
        requirement: *const c_void,
        errors: *mut *mut c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

const CHECK_ALL_ARCHITECTURES: u32 = 1 << 0;
const CHECK_NESTED_CODE: u32 = 1 << 3;
const STRICT_VALIDATE: u32 = 1 << 4;

/// The app must come from this developer, carry this identifier, and be notarized by Apple.
pub fn requirement(bundle_id: &str, team_id: &str) -> String {
    format!(
        "anchor apple generic and identifier \"{bundle_id}\" and certificate leaf[subject.OU] = \"{team_id}\" and notarized"
    )
}

/// Releases a Core Foundation object when dropped.
struct Cf(*const c_void);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

/// Check `app`'s signature, strictly and including nested code, against `requirement`.
/// The error is the Security framework's status code and description, for the log.
pub fn check_signature(app: &Path, requirement: &str) -> Result<(), (i32, String)> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&app.to_string_lossy()));
    let text = NSString::from_str(requirement);
    let mut code: *const c_void = std::ptr::null();
    let mut req: *const c_void = std::ptr::null();
    // SAFETY: NSURL and NSString are toll-free bridged to CFURL and CFString; each
    // call writes a +1 reference (released by Cf) or leaves the out pointer null.
    let status = unsafe { SecStaticCodeCreateWithPath(Retained::as_ptr(&url).cast(), 0, &mut code) };
    let _code = Cf(code);
    if status != 0 {
        return Err((status, "the app couldn't be opened for checking".into()));
    }
    let status = unsafe { SecRequirementCreateWithString(Retained::as_ptr(&text).cast(), 0, &mut req) };
    let _req = Cf(req);
    if status != 0 {
        return Err((status, "the code requirement didn't parse".into()));
    }
    let mut error: *mut c_void = std::ptr::null_mut();
    let flags = CHECK_ALL_ARCHITECTURES | CHECK_NESTED_CODE | STRICT_VALIDATE;
    let status = unsafe { SecStaticCodeCheckValidityWithErrors(code, flags, req, &mut error) };
    if status == 0 {
        return Ok(());
    }
    // SAFETY: a CFErrorRef is an NSError; we own the +1 reference.
    let why = unsafe { Retained::from_raw(error.cast::<NSError>()) }
        .map(|e| e.localizedDescription().to_string())
        .unwrap_or_default();
    Err((status, why))
}

/// errSecCSReqFailed: signed and intact, but by someone else, or not notarized.
pub const REQUIREMENT_FAILED: i32 = -67050;

/// The bundle's CFBundleIdentifier and CFBundleShortVersionString.
pub fn bundle_info(app: &Path) -> Option<(String, String)> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(&app.to_string_lossy()))?;
    let id = bundle.bundleIdentifier()?.to_string();
    let version = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?
        .downcast::<NSString>()
        .ok()?
        .to_string();
    Some((id, version))
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
        let big = vec![b'a'; 1_000_000];
        assert_eq!(hex(sha256(&big[..]).unwrap()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn requirement_names_developer_app_and_notarization() {
        assert_eq!(
            requirement("top.piedol.tamka", "D9JNQPYFBM"),
            "anchor apple generic and identifier \"top.piedol.tamka\" and certificate leaf[subject.OU] = \"D9JNQPYFBM\" and notarized"
        );
    }

    #[test]
    fn unsigned_code_is_refused() {
        let dir = std::env::temp_dir().join(format!("update-verify-{}", std::process::id()));
        let app = dir.join("Fake.app/Contents/MacOS");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Fake"), b"#!/bin/sh\n").unwrap();
        let result = check_signature(&dir.join("Fake.app"), &requirement("top.piedol.tamka", "D9JNQPYFBM"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err());
    }

    /// Against a real release. Download and unpack it, then:
    /// `UPDATE_TEST_APP=/path/to/Tamka.app cargo test real_release -- --ignored`
    #[test]
    #[ignore]
    fn real_release_passes_and_wrong_team_fails() {
        let app = std::path::PathBuf::from(std::env::var("UPDATE_TEST_APP").expect("set UPDATE_TEST_APP"));
        check_signature(&app, &requirement("top.piedol.tamka", "D9JNQPYFBM")).expect("the release should pass");
        let wrong_team = check_signature(&app, &requirement("top.piedol.tamka", "AAAAAAAAAA"));
        assert_eq!(wrong_team.unwrap_err().0, REQUIREMENT_FAILED);
        let wrong_id = check_signature(&app, &requirement("top.piedol.other", "D9JNQPYFBM"));
        assert_eq!(wrong_id.unwrap_err().0, REQUIREMENT_FAILED);
        let (id, version) = bundle_info(&app).unwrap();
        assert_eq!(id, "top.piedol.tamka");
        assert!(super::super::Version::parse(&version).is_some());
    }
}
