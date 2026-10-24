//! The capture overlay window: one borderless topmost window per monitor that
//! forwards input to the session and paints with a Direct2D HWND target.

use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{D2D1_RENDER_TARGET_PROPERTIES, ID2D1DCRenderTarget, ID2D1RenderTarget};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, ValidateRect};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HTCLIENT, IDC_ARROW,
    LoadCursorW, RegisterClassExW, SW_SHOWNA, SetWindowLongPtrW, ShowWindow, WM_CHAR, WM_DESTROY, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SYSKEYDOWN, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use crate::d2d::{self, rgba};
use crate::geometry::{pt, Point, Rect};
use crate::session;

static REGISTER: Once = Once::new();

/// The system accent colour (from DWM), with alpha.
pub fn accent(alpha: f32) -> D2D1_COLOR_F {
    let mut c = 0u32;
    let mut opaque = windows::core::BOOL(0);
    if unsafe { DwmGetColorizationColor(&mut c, &mut opaque) }.is_ok() && c != 0 {
        let r = ((c >> 16) & 0xff) as f32 / 255.0;
        let g = ((c >> 8) & 0xff) as f32 / 255.0;
        let b = (c & 0xff) as f32 / 255.0;
        // Colorization can be very dark or washed out; keep it legible.
        if r + g + b > 0.35 {
            return rgba(r, g, b, alpha);
        }
    }
    rgba(0.0, 0.47, 0.84, alpha)
}

pub fn modifiers() -> (bool, bool, bool) {
    unsafe {
        (
            GetKeyState(VK_SHIFT.0 as i32) < 0,
            GetKeyState(VK_CONTROL.0 as i32) < 0,
            GetKeyState(VK_MENU.0 as i32) < 0,
        )
    }
}

fn lparam_point(l: LPARAM) -> Point {
    let x = (l.0 & 0xffff) as i16 as f64;
    let y = ((l.0 >> 16) & 0xffff) as i16 as f64;
    pt(x, y)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    Down,
    Drag,
    Up,
    Move,
    DoubleClick,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let idx = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as usize;
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            session::paint(idx, hdc);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let dragging = wp.0 & 0x0001 != 0; // MK_LBUTTON
            session::mouse(idx, if dragging { MouseKind::Drag } else { MouseKind::Move }, lparam_point(lp));
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            session::mouse(idx, MouseKind::Down, lparam_point(lp));
            return LRESULT(0);
        }
        WM_LBUTTONDBLCLK => {
            session::mouse(idx, MouseKind::DoubleClick, lparam_point(lp));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            session::mouse(idx, MouseKind::Up, lparam_point(lp));
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            session::right_click();
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as i16;
            session::scroll(delta as f64);
            return LRESULT(0);
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if session::key_down(wp.0 as u16) {
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            if session::char_input(wp.0 as u32) {
                return LRESULT(0);
            }
        }
        WM_SETCURSOR => {
            if (lp.0 & 0xffff) as u32 == HTCLIENT {
                session::set_cursor(idx);
                return LRESULT(1);
            }
        }
        WM_DESTROY => return LRESULT(0),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn register() {
    REGISTER.call_once(|| unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("PhotonOverlay"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

/// One monitor's overlay window plus its render target.
pub struct OverlayWindow {
    pub hwnd: HWND,
    pub rt: Option<ID2D1DCRenderTarget>,
    pub size: (u32, u32),
}

impl OverlayWindow {
    pub fn new(frame: Rect, idx: usize) -> OverlayWindow {
        register();
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                w!("PhotonOverlay"),
                w!("Photon"),
                WS_POPUP,
                frame.x as i32,
                frame.y as i32,
                frame.w as i32,
                frame.h as i32,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .expect("overlay window");
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, idx as isize);
            OverlayWindow { hwnd, rt: None, size: (frame.w as u32, frame.h as u32) }
        }
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        }
    }

    /// A software Direct2D target bound to this paint's DC (created once).
    pub fn target(&mut self, hdc: HDC) -> Option<ID2D1RenderTarget> {
        if self.rt.is_none() {
            let mut props: D2D1_RENDER_TARGET_PROPERTIES = d2d::premultiplied_props();
            props.pixelFormat.alphaMode = windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_IGNORE;
            self.rt = unsafe { d2d::factory().CreateDCRenderTarget(&props) }.ok();
        }
        let rt = self.rt.clone()?;
        let r = RECT { left: 0, top: 0, right: self.size.0 as i32, bottom: self.size.1 as i32 };
        unsafe { rt.BindDC(hdc, &r) }.ok()?;
        windows_core::Interface::cast(&rt).ok()
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn validate(&self) {
        let r = RECT { left: 0, top: 0, right: self.size.0 as i32, bottom: self.size.1 as i32 };
        unsafe {
            let _ = ValidateRect(Some(self.hwnd), Some(&r));
        }
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.rt = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
