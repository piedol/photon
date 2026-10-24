//! Webcam bubble: a round, draggable, mirrored camera preview that IS included
//! in the recording (unlike Photon's other windows).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HTCAPTION, RegisterClassExW, SW_SHOWNOACTIVATE,
    ShowWindow, WM_MOVE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::w;

use super::mf::{self, RgbReader};
use crate::d2d::{self, Layered, rgba};
use crate::geometry::Rect;
use crate::{hud, util};

const SIZE: f64 = 168.0;
const INSET: f64 = 24.0;

static REGISTER: Once = Once::new();

thread_local! {
    static ALIVE: Cell<u64> = const { Cell::new(0) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => return LRESULT(HTCAPTION as isize),
        WM_MOVE => {}
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub struct Bubble {
    hwnd: HWND,
    stop: Arc<AtomicBool>,
    generation: u64,
}

struct Shared {
    frame: Mutex<Option<Vec<u8>>>,
}

impl Bubble {
    pub fn show(region: Rect, scale: f64, device: Option<&str>) -> Option<Bubble> {
        REGISTER.call_once(|| unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: w!("PhotonCam"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
        let size = (SIZE * scale).min(region.w * 0.4).min(region.h * 0.4).max(72.0 * scale).round();
        let r = Rect::new(region.max_x() - size - INSET * scale, region.max_y() - size - INSET * scale, size, size);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("PhotonCam"),
                w!("Photon camera"),
                WS_POPUP,
                r.x as i32,
                r.y as i32,
                size as i32,
                size as i32,
                None,
                None,
                Some(GetModuleHandleW(None).unwrap_or_default().into()),
                None,
            )
            .ok()?
        };
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared { frame: Mutex::new(None) });
        let px = size as u32;
        let device = device.map(String::from);
        {
            let (stop, shared) = (stop.clone(), shared.clone());
            std::thread::spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                }
                mf::startup();
                let Some(reader) = RgbReader::camera(device.as_deref()) else {
                    util::main_async(|| hud::message("Couldn't start the camera", Some("Check Settings › Privacy › Camera and that no other app is using it.")));
                    return;
                };
                while !stop.load(Ordering::Relaxed) {
                    let Some(f) = reader.next() else { break };
                    let mut img = mf::scale_cover(&f, px, px, true);
                    mask_circle(&mut img, px);
                    *shared.frame.lock().unwrap() = Some(img);
                }
            });
        }
        let generation = ALIVE.with(|a| {
            let g = a.get() + 1;
            a.set(g);
            g
        });
        let surface = Layered::new(hwnd, size as i32, size as i32);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        paint_loop(hwnd, surface, shared, px, scale, generation);
        Some(Bubble { hwnd, stop, generation })
    }
}

/// Make pixels outside the circle transparent (premultiplied), with a soft edge.
fn mask_circle(img: &mut [u8], size: u32) {
    let c = size as f64 / 2.0;
    let r = c - 1.0;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            let i = ((y * size + x) * 4) as usize;
            if a < 1.0 {
                for k in 0..3 {
                    img[i + k] = (img[i + k] as f64 * a) as u8;
                }
                img[i + 3] = (255.0 * a) as u8;
            }
        }
    }
}

fn paint_loop(hwnd: HWND, surface: Layered, shared: Arc<Shared>, px: u32, scale: f64, generation: u64) {
    if ALIVE.with(|a| a.get()) != generation {
        return; // bubble closed
    }
    let frame = shared.frame.lock().unwrap().take();
    if let Some(img) = frame {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut r);
        }
        surface.paint(r.left, r.top, 255, |rt| unsafe {
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            if let Ok(bmp) = rt.CreateBitmap(D2D_SIZE_U { width: px, height: px }, Some(img.as_ptr() as _), px * 4, &props) {
                rt.DrawBitmap(&bmp, Some(&d2d::rect(0.0, 0.0, px as f64, px as f64)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
            let c = px as f64 / 2.0;
            let ring = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.95));
            rt.DrawEllipse(&d2d::ellipse(c, c, c - 1.5 * scale, c - 1.5 * scale), &ring, (2.5 * scale) as f32, None);
        });
    }
    let keep = util::MainBound(surface);
    util::main_after(1.0 / 30.0, move || {
        let whole = keep;
        paint_loop(hwnd, whole.0, shared, px, scale, generation);
    });
}

impl Drop for Bubble {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        ALIVE.with(|a| {
            if a.get() == self.generation {
                a.set(self.generation + 1);
            }
        });
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
