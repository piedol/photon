//! The trim window: a preview of the current frame, a filmstrip with yellow
//! start/end handles (like QuickTime), and Cancel / Trim. Trimming itself is a
//! lossless cut (see `apply_trim`).

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Once;
use std::sync::mpsc::{Sender, channel};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES,
    ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IDC_ARROW, IDC_SIZEWE, LoadCursorW, RegisterClassExW,
    SW_SHOW, SetCursor, SetForegroundWindow, ShowWindow, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_THICKFRAME, HTCLIENT,
};
use windows::core::w;
use windows_core::Interface;

use super::super::mf::{self, RgbReader};
use crate::d2d::{self, rgba};
use crate::geometry::{pt, Point, Rect};
use crate::hud;
use crate::keys::vk;
use crate::util;

type Img = (Vec<u8>, u32, u32);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Start,
    End,
    Head,
}

struct State {
    hwnd: HWND,
    rt: Option<windows::Win32::Graphics::Direct2D::ID2D1DCRenderTarget>,
    path: PathBuf,
    duration: f64,
    start: f64,
    end: f64,
    head: f64,
    thumbs: Vec<Option<Img>>,
    preview: Option<Img>,
    drag: Option<Drag>,
    hover: Option<&'static str>,
    req: Sender<f64>,
}

thread_local! {
    static TRIM: RefCell<Option<State>> = const { RefCell::new(None) };
}

static REGISTER: Once = Once::new();
const THUMBS: usize = 12;

fn scale(hwnd: HWND) -> f64 {
    unsafe { GetDpiForWindow(hwnd) as f64 / 96.0 }.max(1.0)
}

struct Layout {
    preview: Rect,
    strip: Rect,
    cancel: Rect,
    trim: Rect,
    s: f64,
}

fn layout(hwnd: HWND) -> Layout {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    let (w, h) = (r.right as f64, r.bottom as f64);
    let s = scale(hwnd);
    let m = 16.0 * s;
    let strip_h = 64.0 * s;
    let bar_h = 44.0 * s;
    let strip = Rect::new(m, h - bar_h - strip_h - m, w - m * 2.0, strip_h);
    let preview = Rect::new(m, m, w - m * 2.0, strip.y - m * 2.0);
    let bw = 96.0 * s;
    let trim = Rect::new(w - m - bw, h - bar_h - 4.0 * s + 6.0 * s, bw, 32.0 * s);
    let cancel = Rect::new(trim.x - 8.0 * s - bw, trim.y, bw, trim.h);
    Layout { preview, strip, cancel, trim, s }
}

fn x_for(t: f64, strip: &Rect, dur: f64) -> f64 {
    strip.x + strip.w * (t / dur.max(0.001))
}

fn t_for(x: f64, strip: &Rect, dur: f64) -> f64 {
    ((x - strip.x) / strip.w).clamp(0.0, 1.0) * dur
}

fn fmt(t: f64) -> String {
    let ms = (t * 1000.0).round() as u64;
    format!("{}:{:02}.{}", ms / 60_000, (ms / 1000) % 60, (ms % 1000) / 100)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let p = pt((lp.0 & 0xffff) as i16 as f64, ((lp.0 >> 16) & 0xffff) as i16 as f64);
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            paint(hwnd, hdc);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_SIZE => {
            TRIM.with(|t| {
                if let Some(st) = t.borrow_mut().as_mut() {
                    st.rt = None;
                }
            });
            invalidate(hwnd);
        }
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            mouse_down(hwnd, p);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            mouse_move(hwnd, p);
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            mouse_up(hwnd, p);
            return LRESULT(0);
        }
        WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
            let over_handle = TRIM.with(|t| t.borrow().as_ref().map(|st| st.hover == Some("handle")).unwrap_or(false));
            unsafe {
                if let Ok(c) = LoadCursorW(None, if over_handle { IDC_SIZEWE } else { IDC_ARROW }) {
                    SetCursor(Some(c));
                }
            }
            return LRESULT(1);
        }
        WM_KEYDOWN => {
            match wp.0 as u16 {
                vk::ESCAPE => close(),
                vk::RETURN => commit(),
                _ => {}
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            close();
            return LRESULT(0);
        }
        WM_DESTROY => return LRESULT(0),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn open(path: PathBuf) {
    if TRIM.with(|t| t.borrow().is_some()) {
        return;
    }
    REGISTER.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: w!("PhotonTrim"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    let Some(reader) = RgbReader::file(&path) else {
        hud::message("Couldn't open the recording", None);
        return;
    };
    let duration = reader.duration().max(0.1);
    drop(reader);
    let c = hud::cursor();
    let (s, work, _) = hud::monitor_at(c);
    let (w, h) = ((960.0 * s).min(work.w * 0.8), (640.0 * s).min(work.h * 0.8));
    let title = format!("Trim · {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let hwnd = unsafe {
        let t = util::W::new(&title);
        match CreateWindowExW(
            Default::default(),
            w!("PhotonTrim"),
            t.p(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME,
            (work.mid().x - w / 2.0) as i32,
            (work.mid().y - h / 2.0) as i32,
            w as i32,
            h as i32,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap_or_default().into()),
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        }
    };
    unsafe {
        let dark = windows::core::BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
    }

    // Preview worker: always decodes the latest requested time.
    let (tx, rx) = channel::<f64>();
    {
        let path = path.clone();
        let lay = layout(hwnd);
        let (pw, ph) = (lay.preview.w as u32, lay.preview.h as u32);
        let thumb_h = (lay.strip.h - 8.0 * lay.s) as u32;
        let thumb_w = (lay.strip.w / THUMBS as f64) as u32;
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            mf::startup();
            let Some(reader) = RgbReader::file(&path) else { return };
            // Filmstrip first.
            for i in 0..THUMBS {
                let t = duration * (i as f64 + 0.5) / THUMBS as f64;
                reader.seek(t);
                if let Some(f) = reader.next() {
                    let img = mf::fit(&f, thumb_w * 2, thumb_h);
                    util::main_async(move || {
                        TRIM.with(|tr| {
                            if let Some(st) = tr.borrow_mut().as_mut() {
                                if i < st.thumbs.len() {
                                    st.thumbs[i] = Some(img);
                                }
                                invalidate(st.hwnd);
                            }
                        })
                    });
                }
            }
            while let Ok(mut t) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    t = newer;
                }
                reader.seek(t);
                if let Some(f) = reader.next() {
                    let img = mf::fit(&f, pw.max(1), ph.max(1));
                    util::main_async(move || {
                        TRIM.with(|tr| {
                            if let Some(st) = tr.borrow_mut().as_mut() {
                                st.preview = Some(img);
                                invalidate(st.hwnd);
                            }
                        })
                    });
                }
            }
        });
    }
    let _ = tx.send(0.0);
    TRIM.with(|t| {
        *t.borrow_mut() = Some(State {
            hwnd,
            rt: None,
            path,
            duration,
            start: 0.0,
            end: duration,
            head: 0.0,
            thumbs: vec![None; THUMBS],
            preview: None,
            drag: None,
            hover: None,
            req: tx,
        })
    });
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn close_for_test() {
    close();
}

fn close() {
    if let Some(st) = TRIM.with(|t| t.borrow_mut().take()) {
        let hwnd = st.hwnd;
        drop(st); // drops the request channel; the worker exits
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        util::release_memory_soon();
    }
}

fn commit() {
    let job = TRIM.with(|t| t.borrow().as_ref().map(|st| (st.path.clone(), st.start, st.end, st.duration)));
    close();
    if let Some((path, start, end, dur)) = job {
        if start <= 0.01 && end >= dur - 0.01 {
            return; // nothing to cut
        }
        super::apply_trim(path, start, end);
    }
}

fn handle_hit(p: Point, lay: &Layout, st: &State) -> Option<Drag> {
    let sx = x_for(st.start, &lay.strip, st.duration);
    let ex = x_for(st.end, &lay.strip, st.duration);
    let slop = 12.0 * lay.s;
    if p.y < lay.strip.y - 6.0 * lay.s || p.y > lay.strip.max_y() + 6.0 * lay.s {
        return None;
    }
    if (p.x - sx).abs() <= slop {
        Some(Drag::Start)
    } else if (p.x - ex).abs() <= slop {
        Some(Drag::End)
    } else if lay.strip.contains(p) {
        Some(Drag::Head)
    } else {
        None
    }
}

fn mouse_down(hwnd: HWND, p: Point) {
    let lay = layout(hwnd);
    if lay.trim.contains(p) {
        commit();
        return;
    }
    if lay.cancel.contains(p) {
        close();
        return;
    }
    TRIM.with(|t| {
        if let Some(st) = t.borrow_mut().as_mut() {
            st.drag = handle_hit(p, &lay, st);
        }
    });
    mouse_move(hwnd, p);
}

fn mouse_move(hwnd: HWND, p: Point) {
    let lay = layout(hwnd);
    TRIM.with(|t| {
        let mut t = t.borrow_mut();
        let Some(st) = t.as_mut() else { return };
        let min_len = 0.2;
        let time = t_for(p.x, &lay.strip, st.duration);
        match st.drag {
            Some(Drag::Start) => {
                st.start = time.min(st.end - min_len).max(0.0);
                st.head = st.start;
                let _ = st.req.send(st.start);
            }
            Some(Drag::End) => {
                st.end = time.max(st.start + min_len).min(st.duration);
                st.head = st.end;
                let _ = st.req.send((st.end - 0.05).max(0.0));
            }
            Some(Drag::Head) => {
                st.head = time.clamp(st.start, st.end);
                let _ = st.req.send(st.head);
            }
            None => {}
        }
        let hover = match handle_hit(p, &lay, st) {
            Some(Drag::Start | Drag::End) => Some("handle"),
            _ if lay.trim.contains(p) => Some("trim"),
            _ if lay.cancel.contains(p) => Some("cancel"),
            _ => None,
        };
        if hover != st.hover || st.drag.is_some() {
            st.hover = hover;
            invalidate(hwnd);
        }
    });
}

fn mouse_up(_hwnd: HWND, _p: Point) {
    TRIM.with(|t| {
        if let Some(st) = t.borrow_mut().as_mut() {
            st.drag = None;
        }
    });
}

fn bitmap(rt: &ID2D1RenderTarget, img: &Img) -> Option<windows::Win32::Graphics::Direct2D::ID2D1Bitmap> {
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    unsafe { rt.CreateBitmap(D2D_SIZE_U { width: img.1, height: img.2 }, Some(img.0.as_ptr() as _), img.1 * 4, &props).ok() }
}

fn paint(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
    let lay = layout(hwnd);
    TRIM.with(|t| {
        let mut t = t.borrow_mut();
        let Some(st) = t.as_mut() else { return };
        if st.rt.is_none() {
            let mut props: D2D1_RENDER_TARGET_PROPERTIES = d2d::premultiplied_props();
            props.pixelFormat.alphaMode = D2D1_ALPHA_MODE_IGNORE;
            st.rt = unsafe { d2d::factory().CreateDCRenderTarget(&props) }.ok();
        }
        let Some(dc_rt) = st.rt.clone() else { return };
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut r);
        }
        if unsafe { dc_rt.BindDC(hdc, &r) }.is_err() {
            return;
        }
        let rt: ID2D1RenderTarget = dc_rt.cast().unwrap();
        let s = lay.s;
        let yellow = rgba(1.0, 0.80, 0.0, 1.0);
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&rgba(0.09, 0.09, 0.10, 1.0)));
            // Preview frame, fitted.
            if let Some(img) = &st.preview
                && let Some(bmp) = bitmap(&rt, img)
            {
                let k = (lay.preview.w / img.1 as f64).min(lay.preview.h / img.2 as f64);
                let (w, h) = (img.1 as f64 * k, img.2 as f64 * k);
                let r = d2d::rect(lay.preview.mid().x - w / 2.0, lay.preview.mid().y - h / 2.0, w, h);
                rt.DrawBitmap(&bmp, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
            // Filmstrip.
            let strip = lay.strip;
            let tw = strip.w / THUMBS as f64;
            rt.FillRoundedRectangle(&d2d::rr(strip.x, strip.y, strip.w, strip.h, 6.0 * s), &d2d::brush(&rt, rgba(0.16, 0.16, 0.18, 1.0)));
            for (i, th) in st.thumbs.iter().enumerate() {
                if let Some(img) = th
                    && let Some(bmp) = bitmap(&rt, img)
                {
                    let cell = Rect::new(strip.x + tw * i as f64, strip.y + 4.0 * s, tw, strip.h - 8.0 * s);
                    // Cover the cell, cropping the thumbnail's sides.
                    let k = (cell.w / img.1 as f64).max(cell.h / img.2 as f64);
                    let (sw, sh) = (cell.w / k, cell.h / k);
                    let src = d2d::rect((img.1 as f64 - sw) / 2.0, (img.2 as f64 - sh) / 2.0, sw, sh);
                    rt.DrawBitmap(&bmp, Some(&d2d::rect(cell.x, cell.y, cell.w, cell.h)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&src));
                }
            }
            let sx = x_for(st.start, &strip, st.duration);
            let ex = x_for(st.end, &strip, st.duration);
            let shade = d2d::brush(&rt, rgba(0.0, 0.0, 0.0, 0.6));
            rt.FillRectangle(&d2d::rect(strip.x, strip.y, sx - strip.x, strip.h), &shade);
            rt.FillRectangle(&d2d::rect(ex, strip.y, strip.max_x() - ex, strip.h), &shade);
            let yb = d2d::brush(&rt, yellow);
            let hw = 10.0 * s;
            rt.FillRectangle(&d2d::rect(sx, strip.y - 2.0 * s, ex - sx, 3.0 * s), &yb);
            rt.FillRectangle(&d2d::rect(sx, strip.max_y() - 1.0 * s, ex - sx, 3.0 * s), &yb);
            for (x, left) in [(sx, true), (ex, false)] {
                let r = d2d::rr(if left { x - hw } else { x }, strip.y - 2.0 * s, hw, strip.h + 4.0 * s, 4.0 * s);
                rt.FillRoundedRectangle(&r, &yb);
                let grip = d2d::brush(&rt, rgba(0.35, 0.25, 0.0, 0.9));
                let gx = if left { x - hw / 2.0 } else { x + hw / 2.0 };
                rt.DrawLine(d2d::v2(gx, strip.mid().y - 8.0 * s), d2d::v2(gx, strip.mid().y + 8.0 * s), &grip, (2.0 * s) as f32, None);
            }
            // Playhead.
            let hx = x_for(st.head, &strip, st.duration);
            rt.FillRectangle(&d2d::rect(hx - 1.0 * s, strip.y - 6.0 * s, 2.0 * s, strip.h + 12.0 * s), &d2d::brush(&rt, rgba(1.0, 1.0, 1.0, 0.95)));
            // Times.
            let sec = rgba(1.0, 1.0, 1.0, 0.65);
            d2d::draw_text(&rt, &fmt(st.start), strip.x, strip.max_y() + 8.0 * s, 12.0 * s, d2d::WEIGHT_MEDIUM, sec);
            let endl = fmt(st.end);
            let ew = d2d::measure(&d2d::layout(&endl, 12.0 * s, d2d::WEIGHT_MEDIUM)).0;
            d2d::draw_text(&rt, &endl, strip.max_x() - ew, strip.max_y() + 8.0 * s, 12.0 * s, d2d::WEIGHT_MEDIUM, sec);
            let len = format!("Keeping {}", fmt(st.end - st.start));
            d2d::draw_text(&rt, &len, strip.x, lay.trim.y + 6.0 * s, 13.0 * s, d2d::WEIGHT_SEMIBOLD, rgba(1.0, 1.0, 1.0, 0.92));
            // Buttons.
            let cancel_fill = if st.hover == Some("cancel") { rgba(1.0, 1.0, 1.0, 0.2) } else { rgba(1.0, 1.0, 1.0, 0.12) };
            rt.FillRoundedRectangle(&d2d::rr(lay.cancel.x, lay.cancel.y, lay.cancel.w, lay.cancel.h, 7.0 * s), &d2d::brush(&rt, cancel_fill));
            d2d::draw_text_centered(&rt, "Cancel", d2d::rect(lay.cancel.x, lay.cancel.y, lay.cancel.w, lay.cancel.h), 13.0 * s, d2d::WEIGHT_MEDIUM, rgba(1.0, 1.0, 1.0, 0.92));
            let trim_fill = if st.hover == Some("trim") { rgba(1.0, 0.86, 0.2, 1.0) } else { yellow };
            rt.FillRoundedRectangle(&d2d::rr(lay.trim.x, lay.trim.y, lay.trim.w, lay.trim.h, 7.0 * s), &d2d::brush(&rt, trim_fill));
            d2d::draw_text_centered(&rt, "Trim", d2d::rect(lay.trim.x, lay.trim.y, lay.trim.w, lay.trim.h), 13.0 * s, d2d::WEIGHT_SEMIBOLD, rgba(0.1, 0.08, 0.0, 1.0));
            if rt.EndDraw(None, None).is_err() {
                st.rt = None;
            }
        }
    });
}
