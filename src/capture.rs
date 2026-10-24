//! Freeze-frame capture of every display with ScreenCaptureKit, plus the
//! on-screen window list used for window snapping.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSScreen, NSWorkspace};
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::{
    kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowOwnerPID, CGImage,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration,
};

use crate::geometry::{cg_to_appkit, Rect};
use crate::util::{self, MainBound};

/// One display at the moment of capture.
pub struct Display {
    pub id: u32,
    /// AppKit global frame, in points.
    pub frame: Rect,
    pub scale: f64,
    pub name: String,
}

pub struct DisplayShot {
    pub display: Display,
    pub image: CFRetained<CGImage>,
}

pub struct Snapshot {
    pub shots: Vec<DisplayShot>,
    /// On-screen windows (AppKit global points), front to back.
    pub windows: Vec<Rect>,
}

pub fn screen_id(screen: &NSScreen) -> u32 {
    let desc = screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    desc.objectForKey(&key)
        .and_then(|v| v.downcast::<NSNumber>().ok())
        .map(|n| n.unsignedIntValue())
        .unwrap_or(0)
}

pub fn displays(mtm: MainThreadMarker) -> Vec<Display> {
    NSScreen::screens(mtm)
        .iter()
        .map(|s| {
            let f = s.frame();
            Display {
                id: screen_id(&s),
                frame: Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height),
                scale: s.backingScaleFactor(),
                name: s.localizedName().to_string(),
            }
        })
        .collect()
}

/// Height of the primary display, for CG ↔ AppKit conversion.
pub fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm).firstObject().map(|s| s.frame().size.height).unwrap_or(0.0)
}

pub fn frontmost_app_name() -> Option<String> {
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    app.localizedName().map(|n| n.to_string())
}

/// Normal app windows currently on screen, front to back, excluding ours.
pub fn window_rects(mtm: MainThreadMarker) -> Vec<Rect> {
    let primary_h = primary_height(mtm);
    let me = std::process::id() as i64;
    let opts = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(opts, 0) else { return Vec::new() };
    // SAFETY: CFArray is toll-free bridged with NSArray; elements are dictionaries.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const NSArray<NSDictionary<NSString, AnyObject>>) };
    let cf_key = |k: &objc2_core_foundation::CFString| -> &NSString {
        // SAFETY: CFString is toll-free bridged with NSString.
        unsafe { &*(k as *const objc2_core_foundation::CFString as *const NSString) }
    };
    let num = |d: &NSDictionary<NSString, AnyObject>, k: &NSString| -> Option<f64> {
        d.objectForKey(k).and_then(|v| v.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    };
    let (k_layer, k_pid, k_alpha, k_bounds) = unsafe {
        (cf_key(kCGWindowLayer), cf_key(kCGWindowOwnerPID), cf_key(kCGWindowAlpha), cf_key(kCGWindowBounds))
    };
    let (kx, ky, kw, kh) = (NSString::from_str("X"), NSString::from_str("Y"), NSString::from_str("Width"), NSString::from_str("Height"));
    let mut out = Vec::new();
    for d in list.iter() {
        if num(&d, k_layer).unwrap_or(1.0) != 0.0 || num(&d, k_pid).map(|p| p as i64) == Some(me) {
            continue;
        }
        if num(&d, k_alpha).unwrap_or(1.0) < 0.05 {
            continue;
        }
        let Some(b) = d.objectForKey(k_bounds).and_then(|v| v.downcast::<NSDictionary>().ok()) else { continue };
        // SAFETY: the bounds dictionary maps NSString keys to NSNumber values.
        let b: Retained<NSDictionary<NSString, AnyObject>> = unsafe { Retained::cast_unchecked(b) };
        let (Some(x), Some(y), Some(w), Some(h)) = (num(&b, &kx), num(&b, &ky), num(&b, &kw), num(&b, &kh)) else { continue };
        if w < 40.0 || h < 30.0 {
            continue;
        }
        out.push(cg_to_appkit(Rect::new(x, y, w, h), primary_h));
    }
    out
}

type Done = Box<dyn FnOnce(Result<Snapshot, String>)>;

thread_local! {
    static PENDING: RefCell<Option<Done>> = const { RefCell::new(None) };
}

struct Collector {
    remaining: usize,
    images: Vec<(u32, CFRetained<CGImage>)>,
    error: Option<String>,
}

/// Capture every display. `done` runs on the main thread.
pub fn capture_all(mtm: MainThreadMarker, done: impl FnOnce(Result<Snapshot, String>) + 'static) {
    if PENDING.with(|p| p.borrow().is_some()) {
        return; // a capture is already in flight
    }
    PENDING.with(|p| *p.borrow_mut() = Some(Box::new(done)));
    let wanted: Vec<(u32, f64)> = displays(mtm).iter().map(|d| (d.id, d.scale)).collect();

    let handler = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let Some(content) = (unsafe { content.as_ref() }) else {
            let msg = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
            finish(Err(msg.unwrap_or_else(|| "Screen capture isn't available.".into())), Vec::new());
            return;
        };
        let sc_displays = unsafe { content.displays() };
        let targets: Vec<(Retained<SCDisplay>, f64)> = sc_displays
            .iter()
            .filter_map(|d| {
                let id = unsafe { d.displayID() };
                wanted.iter().find(|(w, _)| *w == id).map(|(_, s)| (d.clone(), *s))
            })
            .collect();
        if targets.is_empty() {
            finish(Err("No displays to capture.".into()), Vec::new());
            return;
        }
        let collector = Arc::new(Mutex::new(Collector { remaining: targets.len(), images: Vec::new(), error: None }));
        for (display, scale) in targets {
            let id = unsafe { display.displayID() };
            let filter = unsafe {
                SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &NSArray::new())
            };
            let config = unsafe { SCStreamConfiguration::new() };
            unsafe {
                config.setWidth((display.width() as f64 * scale).round() as usize);
                config.setHeight((display.height() as f64 * scale).round() as usize);
                config.setShowsCursor(false);
                config.setCaptureResolution(SCCaptureResolutionType::Best);
            }
            let collector = collector.clone();
            let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
                let mut c = collector.lock().unwrap();
                match NonNull::new(image) {
                    // SAFETY: the handler's image is valid for the call; retain to keep it.
                    Some(img) => c.images.push((id, unsafe { CFRetained::retain(img) })),
                    None => {
                        let msg = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
                        c.error = Some(msg.unwrap_or_else(|| "Capture failed.".into()));
                    }
                }
                c.remaining -= 1;
                if c.remaining == 0 {
                    let images = std::mem::take(&mut c.images);
                    let result = if images.is_empty() { Err(c.error.take().unwrap_or_default()) } else { Ok(()) };
                    drop(c);
                    finish(result, images);
                }
            });
            unsafe {
                SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&block));
            }
        }
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            false, true, &handler,
        );
    }
}

/// Called on an SCK queue; hops to main to build the snapshot.
fn finish(result: Result<(), String>, images: Vec<(u32, CFRetained<CGImage>)>) {
    let payload = MainBound((result, images));
    util::main_async(move || {
        let whole = payload;
        let MainBound((result, images)) = whole;
        let Some(done) = PENDING.with(|p| p.borrow_mut().take()) else { return };
        let mtm = MainThreadMarker::new().unwrap();
        let out = result.map(|()| {
            let mut shots = Vec::new();
            let mut images = images;
            for display in displays(mtm) {
                if let Some(i) = images.iter().position(|(id, _)| *id == display.id) {
                    let (_, image) = images.swap_remove(i);
                    shots.push(DisplayShot { display, image });
                }
            }
            Snapshot { shots, windows: window_rects(mtm) }
        });
        done(out);
    });
}

pub fn cg_rect(r: Rect) -> CGRect {
    CGRect::new(objc2_core_foundation::CGPoint::new(r.x, r.y), objc2_core_foundation::CGSize::new(r.w, r.h))
}

pub fn rect(r: CGRect) -> Rect {
    Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
}
