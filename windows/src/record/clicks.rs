//! Click highlights: a low-level mouse hook shows a soft expanding ring at each
//! click, in a click-through window that the recording does capture.

use std::cell::Cell;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DestroyWindow, HHOOK, MSLLHOOKSTRUCT, SW_SHOWNOACTIVATE, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_RBUTTONDOWN,
};

use crate::d2d::{self, Layered, rgba};
use crate::geometry::Rect;
use crate::{hud, util};

thread_local! {
    static SCALE: Cell<f64> = const { Cell::new(1.0) };
}

pub struct Rings {
    hook: HHOOK,
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && (wp.0 as u32 == WM_LBUTTONDOWN || wp.0 as u32 == WM_RBUTTONDOWN) {
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        let (x, y) = (info.pt.x as f64, info.pt.y as f64);
        // Never block the hook: draw on the next turn of the message loop.
        util::main_async(move || ring(x, y));
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

impl Rings {
    pub fn start(scale: f64) -> Option<Rings> {
        SCALE.with(|s| s.set(scale));
        let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(GetModuleHandleW(None).ok()?.into()), 0).ok()? };
        Some(Rings { hook })
    }
}

impl Drop for Rings {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
        }
    }
}

fn ring(x: f64, y: f64) {
    let s = SCALE.with(|s| s.get());
    let size = (64.0 * s).round();
    let r = Rect::new(x - size / 2.0, y - size / 2.0, size, size);
    // Not excluded from capture: the ring is meant to be in the video.
    let hwnd = hud::create_layered(r, true, false);
    let surface = Layered::new(hwnd, size as i32, size as i32);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    fn frame(hwnd: windows::Win32::Foundation::HWND, surface: Layered, r: Rect, s: f64, step: u32) {
        const STEPS: u32 = 14;
        if step > STEPS {
            drop(surface);
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return;
        }
        let t = step as f64 / STEPS as f64;
        let c = r.w / 2.0;
        let radius = (8.0 + 20.0 * t) * s;
        let alpha = (1.0 - t) as f32;
        surface.paint(r.x as i32, r.y as i32, 255, |rt| unsafe {
            rt.FillEllipse(&d2d::ellipse(c, c, radius, radius), &d2d::brush(rt, rgba(1.0, 0.84, 0.04, 0.25 * alpha)));
            rt.DrawEllipse(&d2d::ellipse(c, c, radius, radius), &d2d::brush(rt, rgba(1.0, 0.84, 0.04, 0.9 * alpha)), (2.5 * s) as f32, None);
        });
        util::main_after(0.022, move || frame(hwnd, surface, r, s, step + 1));
    }
    frame(hwnd, surface, r, s, 0);
}
