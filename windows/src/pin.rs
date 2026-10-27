//! Pin to screen: a capture floating above other windows as a reference.
//! Drag to move, scroll to change opacity, double-click or Esc to close,
//! Ctrl+C to copy, right-click for Copy / Save / Close.

use std::cell::RefCell;
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_DBLCLKS, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos,
    GetWindowRect, HTCAPTION, MF_STRING, RegisterClassExW, SW_SHOW, SetForegroundWindow, ShowWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_MOUSEWHEEL, WM_MOVE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
    WM_NCRBUTTONUP, WM_RBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use crate::d2d::{self, Layered, rgba};
use crate::export;
use crate::hud;
use crate::i18n::tr;
use crate::keys::vk;
use crate::session::Finished;

const SHADOW: f64 = 14.0;

struct Pin {
    hwnd: HWND,
    surface: Layered,
    finished: Finished,
    alpha: f64,
    pad: f64,
}

thread_local! {
    static PINS: RefCell<Vec<Pin>> = const { RefCell::new(Vec::new()) };
}

static REGISTER: Once = Once::new();

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // The whole pin drags the window.
        WM_NCHITTEST => return LRESULT(HTCAPTION as isize),
        WM_NCLBUTTONDBLCLK | WM_LBUTTONDBLCLK => {
            close(hwnd);
            return LRESULT(0);
        }
        WM_NCRBUTTONUP | WM_RBUTTONUP => {
            context_menu(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as i16 as f64;
            PINS.with(|ps| {
                if let Some(p) = ps.borrow_mut().iter_mut().find(|p| p.hwnd == hwnd) {
                    p.alpha = (p.alpha + delta / 120.0 * 0.08).clamp(0.25, 1.0);
                }
            });
            paint(hwnd);
            return LRESULT(0);
        }
        WM_MOVE => {
            paint(hwnd);
        }
        WM_KEYDOWN => {
            let ctrl = crate::overlay::modifiers().1;
            match wp.0 as u16 {
                vk::ESCAPE => close(hwnd),
                vk::C if ctrl => copy(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Whether any capture is pinned to the screen.
pub fn any() -> bool {
    PINS.with(|p| !p.borrow().is_empty())
}

pub fn show(f: Finished) {
    REGISTER.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: w!("PhotonPin"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    let pad = (SHADOW * f.scale).round();
    let r = f.global_rect;
    let (w, h) = (r.w + pad * 2.0, r.h + pad * 2.0);
    let title = crate::util::W::new(&tr("Photon pin"));
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("PhotonPin"),
            title.p(),
            WS_POPUP,
            (r.x - pad) as i32,
            (r.y - pad) as i32,
            w as i32,
            h as i32,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap_or_default().into()),
            None,
        )
        .expect("pin window");
        let surface = Layered::new(hwnd, w as i32, h as i32);
        PINS.with(|ps| ps.borrow_mut().push(Pin { hwnd, surface, finished: f, alpha: 1.0, pad }));
        paint(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn paint(hwnd: HWND) {
    PINS.with(|ps| {
        let ps = ps.borrow();
        let Some(p) = ps.iter().find(|p| p.hwnd == hwnd) else { return };
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut r);
        }
        let img = &p.finished.image;
        let pad = p.pad;
        let s = p.finished.scale;
        p.surface.paint(r.left, r.top, (p.alpha * 255.0) as u8, |rt| unsafe {
            // Soft shadow from stacked translucent rounded rects.
            for i in 0..6 {
                let k = (6 - i) as f64 * 2.0 * s;
                let sh = d2d::brush(rt, rgba(0.0, 0.0, 0.0, 0.035));
                rt.FillRoundedRectangle(&d2d::rr(pad - k, pad - k + 3.0 * s, img.width as f64 + 2.0 * k, img.height as f64 + 2.0 * k, 6.0 * s + k), &sh);
            }
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            if let Ok(bmp) = rt.CreateBitmap(D2D_SIZE_U { width: img.width, height: img.height }, Some(img.pixels.as_ptr() as _), img.width * 4, &props) {
                rt.DrawBitmap(&bmp, Some(&d2d::rect(pad, pad, img.width as f64, img.height as f64)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
            let edge = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.25));
            rt.DrawRectangle(&d2d::rect(pad + 0.5, pad + 0.5, img.width as f64 - 1.0, img.height as f64 - 1.0), &edge, 1.0, None);
        });
    });
}

fn close(hwnd: HWND) {
    let p = PINS.with(|ps| {
        let mut ps = ps.borrow_mut();
        ps.iter().position(|p| p.hwnd == hwnd).map(|i| ps.remove(i))
    });
    if p.is_some() {
        drop(p);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        crate::util::release_memory_soon();
    }
}

fn copy(hwnd: HWND) {
    let ok = PINS.with(|ps| ps.borrow().iter().find(|p| p.hwnd == hwnd).map(|p| export::copy_image_to_clipboard(&p.finished.image)));
    if ok == Some(true) {
        hud::toast(&tr("Copied"), None);
    }
}

fn save(hwnd: HWND) {
    let f = PINS.with(|ps| {
        ps.borrow().iter().find(|p| p.hwnd == hwnd).map(|p| Finished {
            image: p.finished.image.clone(),
            display_name: p.finished.display_name.clone(),
            front_app: p.finished.front_app.clone(),
            global_rect: p.finished.global_rect,
            scale: p.finished.scale,
        })
    });
    if let Some(f) = f {
        export::save_with_dialog(f);
    }
}

fn context_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        // The shortcut after the tab is Windows' accelerator column, not part of the text.
        let items = [(1, format!("{}\tCtrl+C", tr("Copy"))), (2, tr("Save…")), (3, format!("{}\tEsc", tr("Close")))];
        for (id, text) in &items {
            let t = crate::util::W::new(text);
            let _ = AppendMenuW(menu, MF_STRING, *id, t.p());
        }
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), hwnd, None);
        let _ = DestroyMenu(menu);
        match cmd.0 {
            1 => copy(hwnd),
            2 => save(hwnd),
            3 => close(hwnd),
            _ => {}
        }
    }
}
