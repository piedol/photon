//! A stand-in for the `libc` crate on Windows, covering only what Photon's
//! shared `naming.rs` needs (`time`, `tm`, `localtime_r`). It maps them to the
//! Universal C Runtime so the Mac source can be reused unchanged.

#![allow(non_camel_case_types)]

pub type c_int = i32;
pub type time_t = i64;

/// Matches the MSVC `struct tm` layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
}

unsafe extern "C" {
    fn _time64(t: *mut time_t) -> time_t;
    fn _localtime64_s(out: *mut tm, t: *const time_t) -> c_int;
}

/// # Safety
/// `t` must be null or valid for writes.
pub unsafe fn time(t: *mut time_t) -> time_t {
    unsafe { _time64(t) }
}

/// # Safety
/// `t` and `out` must be valid. Returns `out` on success, null on failure.
pub unsafe fn localtime_r(t: *const time_t, out: *mut tm) -> *mut tm {
    if unsafe { _localtime64_s(out, t) } == 0 { out } else { std::ptr::null_mut() }
}
