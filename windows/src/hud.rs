//! Floating translucent surfaces: toasts, the "Recording saved" panel, the
//! countdown, the recording control pill and the region outline. They are
//! layered windows painted with Direct2D and hidden from screen capture.

use std::cell::RefCell;
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_HAND,
    LoadCursorW, RegisterClassExW, SW_SHOWNOACTIVATE, SetCursor, SetWindowDisplayAffinity, ShowWindow, WDA_EXCLUDEFROMCAPTURE,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

use crate::commands::Command;
use crate::d2d::{self, Layered, rgba};
use crate::geometry::{pt, Point, Rect};
use crate::toolbar::{self, Icon};
use crate::util;

pub const WM_MOUSELEAVE: u32 = 0x02A3;

/// Monitor scale and work area at a virtual-screen point.
pub fn monitor_at(p: Point) -> (f64, Rect, Rect) {
    unsafe {
        let m = MonitorFromPoint(POINT { x: p.x as i32, y: p.y as i32 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let w = info.rcWork;
        let f = info.rcMonitor;
        (
            dx as f64 / 96.0,
            Rect::new(w.left as f64, w.top as f64, (w.right - w.left) as f64, (w.bottom - w.top) as f64),
            Rect::new(f.left as f64, f.top as f64, (f.right - f.left) as f64, (f.bottom - f.top) as f64),
        )
    }
}

pub fn cursor() -> Point {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    pt(p.x as f64, p.y as f64)
}

static REGISTER: Once = Once::new();

/// What a panel shows.
pub enum Content {
    Toast { title: String, detail: Option<String>, warn: bool },
    Saved { title: String, detail: String },
    Countdown { n: u32 },
    Pill { text: String, paused: bool },
    Outline { recording: bool },
}

pub struct Button {
    pub rect: Rect, // local pixels
    pub cmd: Command,
    pub icon: Option<Icon>,
    pub label: Option<String>,
    pub tooltip: String,
}

pub struct Panel {
    pub id: u64,
    pub hwnd: HWND,
    surface: Layered,
    pub scale: f64,
    pub content: Content,
    pub buttons: Vec<Button>,
    hover: Option<usize>,
    pub hovered: bool,
    alpha: u8,
    draggable: bool,
}

thread_local! {
    static PANELS: RefCell<Vec<Panel>> = const { RefCell::new(Vec::new()) };
    static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let local = || {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut r);
        }
        let c = cursor();
        pt(c.x - r.left as f64, c.y - r.top as f64)
    };
    match msg {
        WM_NCHITTEST => {
            let p = local();
            let (drag, on_button) = PANELS.with(|ps| {
                ps.borrow().iter().find(|p| p.hwnd == hwnd).map(|pn| (pn.draggable, pn.buttons.iter().any(|b| b.rect.contains(p)))).unwrap_or((false, false))
            });
            return LRESULT(if drag && !on_button { HTCAPTION as isize } else { HTCLIENT as isize });
        }
        WM_MOUSEMOVE => {
            let p = local();
            let changed = PANELS.with(|ps| {
                let mut ps = ps.borrow_mut();
                let Some(pn) = ps.iter_mut().find(|p| p.hwnd == hwnd) else { return false };
                let h = pn.buttons.iter().position(|b| b.rect.contains(p));
                let changed = h != pn.hover || !pn.hovered;
                pn.hover = h;
                pn.hovered = true;
                changed
            });
            let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut t);
            }
            if changed {
                repaint_hwnd(hwnd);
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            PANELS.with(|ps| {
                if let Some(pn) = ps.borrow_mut().iter_mut().find(|p| p.hwnd == hwnd) {
                    pn.hover = None;
                    pn.hovered = false;
                }
            });
            repaint_hwnd(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let p = local();
            let cmd = PANELS.with(|ps| ps.borrow().iter().find(|pn| pn.hwnd == hwnd).and_then(|pn| pn.buttons.iter().find(|b| b.rect.contains(p)).map(|b| b.cmd)));
            if let Some(cmd) = cmd {
                crate::commands::run(cmd);
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            let p = local();
            let on = PANELS.with(|ps| ps.borrow().iter().find(|pn| pn.hwnd == hwnd).is_some_and(|pn| pn.buttons.iter().any(|b| b.rect.contains(p))));
            unsafe {
                if let Ok(h) = LoadCursorW(None, if on { IDC_HAND } else { IDC_ARROW }) {
                    SetCursor(Some(h));
                }
            }
            return LRESULT(1);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// A topmost, non-activating layered window. `click_through` ignores the mouse.
pub fn create_layered(r: Rect, click_through: bool, exclude_from_capture: bool) -> HWND {
    REGISTER.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: w!("PhotonHud"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    unsafe {
        let mut ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if click_through {
            ex |= WS_EX_TRANSPARENT;
        }
        let hwnd = CreateWindowExW(
            ex,
            w!("PhotonHud"),
            w!("Photon"),
            WS_POPUP,
            r.x as i32,
            r.y as i32,
            r.w as i32,
            r.h as i32,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap_or_default().into()),
            None,
        )
        .expect("hud window");
        // Developer mode keeps HUDs visible to capture so self-test screenshots show them.
        if exclude_from_capture && std::env::var_os("PHOTON_DEV").is_none() {
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
        hwnd
    }
}

/// Create and show a panel; returns its id.
pub fn show(r: Rect, scale: f64, content: Content, buttons: Vec<Button>, click_through: bool, draggable: bool) -> u64 {
    let hwnd = create_layered(r, click_through, true);
    let id = NEXT.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    let panel = Panel {
        id,
        hwnd,
        surface: Layered::new(hwnd, r.w as i32, r.h as i32),
        scale,
        content,
        buttons,
        hover: None,
        hovered: false,
        alpha: 255,
        draggable,
    };
    PANELS.with(|ps| ps.borrow_mut().push(panel));
    repaint(id);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    id
}

pub fn exists(id: u64) -> bool {
    PANELS.with(|ps| ps.borrow().iter().any(|p| p.id == id))
}

pub fn is_hovered(id: u64) -> bool {
    PANELS.with(|ps| ps.borrow().iter().any(|p| p.id == id && p.hovered))
}

pub fn update(id: u64, f: impl FnOnce(&mut Panel)) {
    PANELS.with(|ps| {
        if let Some(p) = ps.borrow_mut().iter_mut().find(|p| p.id == id) {
            f(p);
        }
    });
    repaint(id);
}

fn repaint_hwnd(hwnd: HWND) {
    let id = PANELS.with(|ps| ps.borrow().iter().find(|p| p.hwnd == hwnd).map(|p| p.id));
    if let Some(id) = id {
        repaint(id);
    }
}

pub fn repaint(id: u64) {
    PANELS.with(|ps| {
        let ps = ps.borrow();
        let Some(p) = ps.iter().find(|p| p.id == id) else { return };
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(p.hwnd, &mut r);
        }
        p.surface.paint(r.left, r.top, p.alpha, |rt| draw(rt, p));
    });
}

pub fn close(id: u64) {
    let p = PANELS.with(|ps| {
        let mut ps = ps.borrow_mut();
        ps.iter().position(|p| p.id == id).map(|i| ps.remove(i))
    });
    if let Some(p) = p {
        let hwnd = p.hwnd;
        drop(p);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Fade out over ~150 ms, then close.
pub fn fade_out(id: u64) {
    fn step(id: u64, n: u8) {
        if !exists(id) {
            return;
        }
        if n == 0 {
            close(id);
            return;
        }
        PANELS.with(|ps| {
            if let Some(p) = ps.borrow_mut().iter_mut().find(|p| p.id == id) {
                p.alpha = (n as u32 * 255 / 6) as u8;
            }
        });
        repaint(id);
        util::main_after(0.025, move || step(id, n - 1));
    }
    step(id, 5);
}

/// Dismiss after `secs`, waiting while the pointer is over it.
pub fn dismiss_after(id: u64, secs: f64) {
    util::main_after(secs, move || {
        if is_hovered(id) {
            dismiss_after(id, 1.5);
        } else {
            fade_out(id);
        }
    });
}

fn draw(rt: &ID2D1RenderTarget, p: &Panel) {
    let s = p.scale;
    let (w, h) = p.surface.size();
    let (w, h) = (w as f64, h as f64);
    let panel = d2d::brush(rt, rgba(0.12, 0.12, 0.14, 0.98));
    let edge = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.10));
    let white = rgba(1.0, 1.0, 1.0, 0.95);
    let secondary = rgba(1.0, 1.0, 1.0, 0.6);
    unsafe {
        match &p.content {
            Content::Toast { title, detail, warn } => {
                let r = d2d::rr(0.5, 0.5, w - 1.0, h - 1.0, 10.0 * s);
                rt.FillRoundedRectangle(&r, &panel);
                rt.DrawRoundedRectangle(&r, &edge, 1.0, None);
                let c = if *warn { rgba(1.0, 0.62, 0.04, 1.0) } else { rgba(0.19, 0.82, 0.35, 1.0) };
                let cy = h / 2.0;
                let b = d2d::brush(rt, c);
                rt.FillEllipse(&d2d::ellipse(22.0 * s, cy, 9.0 * s, 9.0 * s), &b);
                let ink = d2d::brush(rt, rgba(0.08, 0.08, 0.08, 1.0));
                if *warn {
                    rt.DrawLine(d2d::v2(22.0 * s, cy - 4.5 * s), d2d::v2(22.0 * s, cy + 1.0 * s), &ink, (2.0 * s) as f32, &d2d::round_stroke());
                    rt.FillEllipse(&d2d::ellipse(22.0 * s, cy + 4.0 * s, 1.2 * s, 1.2 * s), &ink);
                } else if let Some(g) = d2d::polyline(&[(17.5 * s, cy), (21.0 * s, cy + 3.5 * s), (26.5 * s, cy - 3.5 * s)], false) {
                    rt.DrawGeometry(&g, &ink, (2.0 * s) as f32, &d2d::round_stroke());
                }
                match detail {
                    Some(d) => {
                        d2d::draw_text(rt, title, 40.0 * s, 8.0 * s, 13.0 * s, d2d::WEIGHT_SEMIBOLD, white);
                        d2d::draw_text(rt, d, 40.0 * s, 27.0 * s, 11.5 * s, d2d::WEIGHT_REGULAR, secondary);
                    }
                    None => {
                        d2d::draw_text(rt, title, 40.0 * s, cy - 9.5 * s, 13.0 * s, d2d::WEIGHT_SEMIBOLD, white);
                    }
                }
            }
            Content::Saved { title, detail } => {
                let r = d2d::rr(0.5, 0.5, w - 1.0, h - 1.0, 10.0 * s);
                rt.FillRoundedRectangle(&r, &panel);
                rt.DrawRoundedRectangle(&r, &edge, 1.0, None);
                d2d::draw_text(rt, title, 16.0 * s, 12.0 * s, 13.0 * s, d2d::WEIGHT_SEMIBOLD, white);
                d2d::draw_text(rt, &ellipsize(detail, 46), 16.0 * s, 31.0 * s, 11.5 * s, d2d::WEIGHT_REGULAR, secondary);
            }
            Content::Countdown { n } => {
                let r = d2d::rr(0.5, 0.5, w - 1.0, h - 1.0, 22.0 * s);
                rt.FillRoundedRectangle(&r, &panel);
                rt.DrawRoundedRectangle(&r, &edge, 1.0, None);
                d2d::draw_text_centered(rt, &n.to_string(), d2d::rect(0.0, 10.0 * s, w, 70.0 * s), 50.0 * s, d2d::WEIGHT_SEMIBOLD, white);
            }
            Content::Pill { text, paused } => {
                let r = d2d::rr(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0);
                rt.FillRoundedRectangle(&r, &panel);
                rt.DrawRoundedRectangle(&r, &edge, 1.0, None);
                let dot = if *paused { rgba(1.0, 1.0, 1.0, 0.5) } else { rgba(1.0, 0.27, 0.23, 1.0) };
                rt.FillEllipse(&d2d::ellipse(18.0 * s, h / 2.0, 5.5 * s, 5.5 * s), &d2d::brush(rt, dot));
                d2d::draw_text(rt, text, 30.0 * s, h / 2.0 - 9.0 * s, 13.0 * s, d2d::WEIGHT_MEDIUM, white);
            }
            Content::Outline { recording } => {
                let c = if *recording { rgba(1.0, 0.27, 0.23, 0.95) } else { rgba(1.0, 1.0, 1.0, 0.6) };
                let dash = d2d::dashed_stroke(&[4.0, 2.7]);
                let k = 2.0 * s;
                rt.DrawRectangle(&d2d::rect(k, k, w - 2.0 * k, h - 2.0 * k), &d2d::brush(rt, c), (1.5 * s) as f32, &dash);
            }
        }
        // Buttons.
        for (i, b) in p.buttons.iter().enumerate() {
            let hovered = p.hover == Some(i);
            if let Some(label) = &b.label {
                let fill = if hovered { rgba(1.0, 1.0, 1.0, 0.22) } else { rgba(1.0, 1.0, 1.0, 0.14) };
                rt.FillRoundedRectangle(&d2d::rr(b.rect.x, b.rect.y, b.rect.w, b.rect.h, 7.0 * s), &d2d::brush(rt, fill));
                d2d::draw_text_centered(rt, label, d2d::rect(b.rect.x, b.rect.y, b.rect.w, b.rect.h), 13.0 * s, d2d::WEIGHT_MEDIUM, white);
            } else if let Some(icon) = b.icon {
                if hovered {
                    rt.FillRoundedRectangle(&d2d::rr(b.rect.x + 2.0 * s, b.rect.y + 2.0 * s, b.rect.w - 4.0 * s, b.rect.h - 4.0 * s, 6.0 * s), &d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.14)));
                }
                let c = b.rect.mid();
                toolbar::icon_colored(rt, icon, c.x, c.y, s, if icon == Icon::Stop { rgba(1.0, 0.35, 0.3, 1.0) } else { white });
            }
        }
        // Tooltip for the hovered button (above it, inside the panel's own window is too
        // small, so the tooltip shows as a short label in the title area when present).
        if let (Some(i), Content::Saved { .. }) = (p.hover, &p.content) {
            let t = &p.buttons[i].tooltip;
            let l = d2d::layout(t, 11.0 * s, d2d::WEIGHT_REGULAR);
            let (tw, _) = d2d::measure(&l);
            d2d::draw_text(rt, t, w - tw - 14.0 * s, h - 16.0 * s - 40.0 * s, 11.0 * s, d2d::WEIGHT_REGULAR, secondary);
        }
    }
}

/// Shorten `s` to about `max` columns with an ellipsis in the middle. Chinese, Japanese and
/// Korean characters take two columns.
fn ellipsize(s: &str, max: usize) -> String {
    let cols = |c: char| if is_wide(c) { 2 } else { 1 };
    if s.chars().map(cols).sum::<usize>() <= max {
        return s.to_string();
    }
    let half = max / 2 - 1;
    let mut head = String::new();
    let mut used = 0;
    for c in s.chars() {
        if used + cols(c) > half {
            break;
        }
        used += cols(c);
        head.push(c);
    }
    let mut tail = Vec::new();
    used = 0;
    for c in s.chars().rev() {
        if used + cols(c) > half {
            break;
        }
        used += cols(c);
        tail.push(c);
    }
    let tail: String = tail.into_iter().rev().collect();
    format!("{head}…{tail}")
}

/// East Asian wide characters: Hangul, CJK ideographs and kana, full-width forms.
fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// A quiet confirmation near the pointer.
pub fn toast(title: &str, detail: Option<&str>) {
    show_toast(title, detail, false, if detail.is_some() { 2.2 } else { 0.9 });
}

/// An error or notice that stays a little longer.
pub fn message(title: &str, detail: Option<&str>) {
    show_toast(title, detail, true, 4.0);
}

fn show_toast(title: &str, detail: Option<&str>, warn: bool, secs: f64) {
    let c = cursor();
    let (s, work, _) = monitor_at(c);
    let tw = d2d::measure(&d2d::layout(title, 13.0 * s, d2d::WEIGHT_SEMIBOLD)).0;
    let dw = detail.map(|d| d2d::measure(&d2d::layout(d, 11.5 * s, d2d::WEIGHT_REGULAR)).0.min(560.0 * s)).unwrap_or(0.0);
    let w = 40.0 * s + tw.max(dw) + 16.0 * s;
    let h = if detail.is_some() { 50.0 * s } else { 36.0 * s };
    let r = Rect::new(c.x + 16.0 * s, c.y + 18.0 * s, w, h).clamp_inside(&work.inset(8.0 * s));
    let id = show(
        r,
        s,
        Content::Toast { title: title.into(), detail: detail.map(String::from), warn },
        Vec::new(),
        true,
        false,
    );
    dismiss_after(id, secs);
}
