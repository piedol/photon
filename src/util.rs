//! Small helpers shared across modules.

use std::path::PathBuf;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2_foundation::NSString;

/// Run `f` on the main queue soon.
pub fn main_async(f: impl FnOnce() + Send + 'static) {
    DispatchQueue::main().exec_async(f);
}

/// Run `f` on the main queue after `secs` seconds.
pub fn main_after(secs: f64, f: impl FnOnce() + Send + 'static) {
    let when = DispatchTime::NOW.time((secs * 1_000_000_000.0) as i64);
    let _ = DispatchQueue::main().after(when, f);
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

/// `~/…` display form of a path.
pub fn tilde(path: &std::path::Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Wrapper that lets main-thread-only values ride through `Send` closures that
/// are guaranteed to run back on the main queue.
pub struct MainBound<T>(pub T);
// SAFETY: only used for values that are created on and consumed on the main thread.
unsafe impl<T> Send for MainBound<T> {}

pub fn log(msg: impl AsRef<str>) {
    if cfg!(debug_assertions) || std::env::var_os("PHOTON_LOG").is_some() {
        eprintln!("[photon] {}", msg.as_ref());
    }
}

unsafe extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

/// Hand freed capture buffers back to the system shortly after a session ends,
/// so the idle footprint returns to baseline instead of staying at the peak.
pub fn release_memory_soon() {
    main_after(1.0, || {
        // SAFETY: a null zone means "all zones"; goal 0 means "as much as possible".
        unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    });
}
