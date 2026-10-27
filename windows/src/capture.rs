//! Freeze-frame capture of every monitor (GDI BitBlt in physical pixels) plus
//! the on-screen window list used for click-to-select.

use std::path::Path;

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};
use windows::core::PWSTR;

use crate::geometry::Rect;
use crate::util;

#[derive(Clone, Debug)]
pub struct Display {
    pub handle: isize,
    /// Virtual-screen rect in physical pixels.
    pub frame: Rect,
    /// DPI / 96.
    pub scale: f64,
    pub name: String,
}

pub struct DisplayShot {
    pub display: Display,
    /// BGRA, top-down, `frame.w × frame.h`.
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub hwnd: isize,
    /// Physical-pixel virtual-screen rect (visible frame, without shadows).
    pub rect: Rect,
    pub app: Option<String>,
}

pub struct Snapshot {
    pub shots: Vec<DisplayShot>,
    pub windows: Vec<WindowInfo>,
}

pub fn displays() -> Vec<Display> {
    unsafe extern "system" fn cb(m: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<Display>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(m, &mut info as *mut _ as *mut _) }.as_bool() {
            let r = info.monitorInfo.rcMonitor;
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
            let name = util::from_wide(&info.szDevice).trim_start_matches("\\\\.\\").to_string();
            out.push(Display {
                handle: m.0 as isize,
                frame: Rect::new(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64),
                scale: dx as f64 / 96.0,
                name,
            });
        }
        BOOL(1)
    }
    let mut out: Vec<Display> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Copy a virtual-screen rect into a BGRA buffer.
pub fn grab(r: Rect) -> Option<Vec<u8>> {
    let (x, y, w, h) = (r.x as i32, r.y as i32, r.w as i32, r.h as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr() as _), &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok || lines == 0 {
            return None;
        }
        // GDI leaves alpha at 0; make the frame opaque.
        for a in px.iter_mut().skip(3).step_by(4) {
            *a = 255;
        }
        Some(px)
    }
}

/// Capture every monitor plus the window list. Runs synchronously (~20–40 ms).
pub fn capture_all() -> Result<Snapshot, String> {
    let windows = window_rects();
    let mut shots = Vec::new();
    for d in displays() {
        if let Some(pixels) = grab(d.frame) {
            shots.push(DisplayShot { display: d, pixels });
        }
    }
    if shots.is_empty() {
        return Err(crate::i18n::tr("Couldn't read the screen."));
    }
    Ok(Snapshot { shots, windows })
}

/// The app's display name from its version info ("Google Chrome", "Discord").
fn file_description(path: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let p = util::W::new(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(p.p(), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(p.p(), None, size, data.as_mut_ptr() as _).ok()?;
        // First language/codepage pair.
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let q = util::W::new("\\VarFileInfo\\Translation");
        if !VerQueryValueW(data.as_ptr() as _, q.p(), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *(ptr as *const u16).add(1);
        let key = util::W::new(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as _, key.p(), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let stem = Path::new(&path).file_stem()?.to_string_lossy().into_owned();
        if stem.eq_ignore_ascii_case("ApplicationFrameHost") {
            return None; // Store apps: the caller uses the window title instead
        }
        if let Some(desc) = file_description(&path).filter(|d| d.len() <= 40) {
            return Some(desc);
        }
        // "discord" → "Discord", "msedge" → "Msedge": good enough for file names.
        let mut c = stem.chars();
        Some(match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => stem,
        })
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = vec![0u16; 256];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf) };
    (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
}

pub fn window_app(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    process_name(pid).or_else(|| window_title(hwnd))
}

pub fn frontmost_app_name() -> Option<String> {
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() { None } else { window_app(h) }
}

/// Visible app windows, front to back, excluding Photon's own.
pub fn window_rects() -> Vec<WindowInfo> {
    unsafe extern "system" fn cb(h: HWND, data: LPARAM) -> BOOL {
        let out = unsafe { &mut *(data.0 as *mut Vec<WindowInfo>) };
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                return BOOL(1);
            }
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 || GetWindowTextLengthW(h) == 0 {
                return BOOL(1);
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as _, 4);
            if cloaked != 0 {
                return BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == std::process::id() {
                return BOOL(1);
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as _, std::mem::size_of::<RECT>() as u32).is_err() {
                return BOOL(1);
            }
            let (w, hh) = (r.right - r.left, r.bottom - r.top);
            if w < 60 || hh < 40 {
                return BOOL(1);
            }
            out.push(WindowInfo {
                hwnd: h.0 as isize,
                rect: Rect::new(r.left as f64, r.top as f64, w as f64, hh as f64),
                app: process_name(pid).or_else(|| window_title(h)),
            });
        }
        BOOL(1)
    }
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}
