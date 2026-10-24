//! After a recording: join pause segments, the "Recording saved" HUD, trimming
//! (the system QuickTime-style trimmer), and GIF export.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSImageView, NSPanel, NSPasteboard, NSTextField, NSWindow,
    NSWindowStyleMask, NSWorkspace, NSFontWeightSemibold,
};
use objc2_av_foundation::{
    AVAsset, AVAssetExportPresetPassthrough, AVAssetExportSession, AVAssetExportSessionStatus,
    AVAssetImageGenerator, AVFileTypeMPEG4, AVMutableComposition, AVPlayer, AVURLAsset,
};
use objc2_av_kit::{AVPlayerView, AVPlayerViewControlsStyle, AVPlayerViewTrimResult};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFURL, CGSize};
use objc2_core_media::{kCMTimeZero, CMTime, CMTimeRange};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL};
use objc2_image_io::{
    kCGImagePropertyGIFDelayTime, kCGImagePropertyGIFDictionary, kCGImagePropertyGIFLoopCount, CGImageDestination,
};

use crate::capture::cg_rect;
use crate::commands::Command;
use crate::geometry::Rect;
use crate::output::destination;
use crate::util::{self, ns, MainBound};
use crate::{hud, ui};

fn file_url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&ns(&p.to_string_lossy()))
}

fn asset(p: &Path) -> Retained<AVURLAsset> {
    unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(p), None) }
}

/// Export `asset` (optionally a time range) to an MP4 without re-encoding.
fn passthrough(asset: &AVAsset, range: Option<CMTimeRange>, out: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    let Some(export) = (unsafe {
        AVAssetExportSession::initWithAsset_presetName(AVAssetExportSession::alloc(), asset, AVAssetExportPresetPassthrough)
    }) else {
        return done(Err("Couldn't prepare the export.".into()));
    };
    unsafe {
        export.setOutputURL(Some(&file_url(&out)));
        export.setOutputFileType(AVFileTypeMPEG4);
        if let Some(r) = range {
            export.setTimeRange(r);
        }
    }
    let id = NEXT_JOB.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    JOBS.with(|j| j.borrow_mut().push((id, Box::new(done))));
    let session = MainBound(export.clone());
    let block = RcBlock::new(move || {
        let status = unsafe { session.0.status() };
        let err = unsafe { session.0.error() }.map(|e| e.localizedDescription().to_string());
        let result = if status == AVAssetExportSessionStatus::Completed {
            Ok(out.clone())
        } else {
            Err(err.unwrap_or_else(|| "Export failed.".into()))
        };
        util::main_async(move || {
            let job = JOBS.with(|j| {
                let mut j = j.borrow_mut();
                j.iter().position(|(i, _)| *i == id).map(|i| j.remove(i).1)
            });
            if let Some(f) = job {
                f(result);
            }
        });
    });
    unsafe { export.exportAsynchronouslyWithCompletionHandler(&block) };
}

// Export completions arrive on a background queue and hop back to main by job id.
static NEXT_JOB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
type Job = Box<dyn FnOnce(Result<PathBuf, String>)>;
thread_local! {
    static JOBS: RefCell<Vec<(u64, Job)>> = const { RefCell::new(Vec::new()) };
}

/// Join pause segments into one file (passthrough). One segment is used as is.
pub fn join(segments: Vec<PathBuf>, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    if segments.len() == 1 {
        return done(Ok(segments.into_iter().next().unwrap()));
    }
    let comp = unsafe { AVMutableComposition::composition() };
    let mut cursor = unsafe { kCMTimeZero };
    for seg in &segments {
        let a = asset(seg);
        let dur = unsafe { a.duration() };
        let range = CMTimeRange { start: unsafe { kCMTimeZero }, duration: dur };
        if let Err(e) = unsafe { comp.insertTimeRange_ofAsset_atTime_error(range, &a, cursor) } {
            util::log(format!("skipping segment {}: {e:?}", seg.display()));
            continue;
        }
        cursor = unsafe { cursor.add(dur) };
    }
    let out = destination::scratch_dir().join(format!("joined-{}.mp4", std::process::id()));
    let _ = std::fs::remove_file(&out);
    passthrough(&comp, None, out, move |r| {
        for s in segments {
            let _ = std::fs::remove_file(s);
        }
        done(r);
    });
}

// ── Finished HUD ────────────────────────────────────────────────────────────

struct Finished {
    path: PathBuf,
    hud_id: u64,
}

thread_local! {
    static LAST: RefCell<Option<Finished>> = const { RefCell::new(None) };
}

/// "Recording saved" with Reveal / Copy / Open / Trim / GIF.
pub fn finished_hud(path: PathBuf, fell_back: bool) {
    let mtm = MainThreadMarker::new().unwrap();
    dismiss_finished();
    let is_video = path.extension().is_some_and(|e| e == "mp4" || e == "mov");
    let actions: Vec<(&str, &str, Command)> = {
        let mut v = vec![
            ("folder", "Show in Finder", Command::Reveal),
            ("doc.on.doc", "Copy file", Command::CopyFile),
            ("play.rectangle", "Open", Command::OpenFile),
        ];
        if is_video {
            v.push(("timeline.selection", "Trim", Command::Trim));
            v.push(("photo.stack", "Make GIF", Command::ExportGif));
        }
        v
    };
    let btn = 30.0;
    let w = (24.0 + btn * actions.len() as f64 + 30.0).max(300.0);
    let h = 88.0;
    let screen = hud::pointer_screen(mtm);
    let frame = Rect::new(screen.max_x() - w - 16.0, screen.y + 16.0, w, h);
    let panel = hud::panel(mtm, frame, objc2_app_kit::NSStatusWindowLevel);
    let surface = ui::hud_surface(mtm, cg_rect(Rect::new(0.0, 0.0, w, h)), 12.0, false);

    let title = NSTextField::labelWithString(
        &ns(if fell_back { "Saved locally · cloud folder missing" } else if is_video { "Recording saved" } else { "GIF saved" }),
        mtm,
    );
    title.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe { NSFontWeightSemibold })));
    title.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.95)));
    title.setFrame(cg_rect(Rect::new(16.0, h - 30.0, w - 50.0, 17.0)));
    surface.addSubview(&title);

    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let detail = NSTextField::labelWithString(&ns(&format!("{name} · {}", util::tilde(path.parent().unwrap_or(&path)))), mtm);
    detail.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    detail.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.6)));
    detail.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
    detail.setFrame(cg_rect(Rect::new(16.0, h - 48.0, w - 32.0, 15.0)));
    surface.addSubview(&detail);

    let close = ui::icon_button(mtm, "xmark", "Dismiss", Command::DismissHud);
    close.setFrame(cg_rect(Rect::new(w - 34.0, h - 32.0, 24.0, 24.0)));
    surface.addSubview(&close);

    let mut x = 10.0;
    for (sym, tip, cmd) in actions {
        let b = ui::icon_button(mtm, sym, tip, cmd);
        b.setFrame(cg_rect(Rect::new(x, 8.0, btn, btn)));
        surface.addSubview(&b);
        x += btn + 2.0;
    }
    if let Some(img) = ui::symbol("checkmark.circle.fill", 13.0) {
        let iv = NSImageView::imageViewWithImage(&img, mtm);
        iv.setContentTintColor(Some(&NSColor::systemGreenColor()));
        iv.setFrame(cg_rect(Rect::new(w - 58.0, h - 31.0, 18.0, 18.0)));
        let _ = iv; // the title carries the state; keep the HUD quiet
    }
    panel.setContentView(Some(&surface));
    panel.orderFrontRegardless();
    let hud_id = hud::keep(panel);
    hud::dismiss_after(hud_id, 10.0);
    LAST.with(|l| *l.borrow_mut() = Some(Finished { path, hud_id }));
}

fn dismiss_finished() {
    if let Some(f) = LAST.with(|l| l.borrow_mut().take()) {
        hud::fade_out(f.hud_id);
    }
}

fn last_path() -> Option<PathBuf> {
    LAST.with(|l| l.borrow().as_ref().map(|f| f.path.clone()))
}

/// HUD button commands.
pub fn command(cmd: Command) {
    let Some(path) = last_path() else { return };
    match cmd {
        Command::Reveal => {
            let urls = NSArray::from_retained_slice(&[file_url(&path)]);
            NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
            dismiss_finished();
        }
        Command::CopyFile => {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let url = file_url(&path);
            let objs: Retained<NSArray<AnyObject>> = unsafe { Retained::cast_unchecked(NSArray::from_retained_slice(&[url])) };
            let objs: Retained<NSArray<objc2::runtime::ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>> = unsafe { Retained::cast_unchecked(objs) };
            pb.writeObjects(&objs);
            dismiss_finished();
            hud::toast("File copied", Some("Paste it into Discord, Slack or Finder"));
        }
        Command::OpenFile => {
            NSWorkspace::sharedWorkspace().openURL(&file_url(&path));
            dismiss_finished();
        }
        Command::Trim => {
            dismiss_finished();
            trim(path);
        }
        Command::ExportGif => {
            dismiss_finished();
            make_gif(path);
        }
        Command::DismissHud => dismiss_finished(),
        _ => {}
    }
}

// ── Trim ───────────────────────────────────────────────────────────────────

thread_local! {
    static TRIM: RefCell<Option<(Retained<NSWindow>, Retained<AVPlayerView>, Retained<AVPlayer>)>> = const { RefCell::new(None) };
}

fn trim(path: PathBuf) {
    let mtm = MainThreadMarker::new().unwrap();
    if TRIM.with(|t| t.borrow().is_some()) {
        return;
    }
    let player = unsafe { AVPlayer::playerWithURL(&file_url(&path), mtm) };
    let screen = hud::pointer_screen(mtm);
    let (w, h) = (min(900.0, screen.w * 0.7), min(600.0, screen.h * 0.7));
    let frame = Rect::new(screen.mid().x - w / 2.0, screen.mid().y - h / 2.0, w, h);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            cg_rect(frame),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&ns(&format!("Trim · {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
    window.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    let view = unsafe { AVPlayerView::initWithFrame(AVPlayerView::alloc(mtm), cg_rect(Rect::new(0.0, 0.0, w, h))) };
    unsafe {
        view.setPlayer(Some(&player));
        view.setControlsStyle(AVPlayerViewControlsStyle::Floating);
    }
    window.setContentView(Some(&view));
    ui::app(mtm).activate();
    window.makeKeyAndOrderFront(None);
    TRIM.with(|t| *t.borrow_mut() = Some((window, view.clone(), player.clone())));

    // Trimming can only begin once the item is ready; retry briefly.
    begin_trim_when_ready(path, 0);
}

fn min(a: f64, b: f64) -> f64 {
    a.min(b)
}

fn begin_trim_when_ready(path: PathBuf, attempt: u32) {
    let ready = TRIM.with(|t| t.borrow().as_ref().map(|(_, v, _)| unsafe { v.canBeginTrimming() })).unwrap_or(false);
    if !ready {
        if attempt < 40 {
            util::main_after(0.1, move || begin_trim_when_ready(path, attempt + 1));
        } else {
            close_trim();
            hud::message("Couldn't open the trimmer", None);
        }
        return;
    }
    let view = TRIM.with(|t| t.borrow().as_ref().map(|(_, v, _)| v.clone()));
    let Some(view) = view else { return };
    let p = MainBound(path);
    let block = RcBlock::new(move |result: AVPlayerViewTrimResult| {
        let path = p.0.clone();
        let ok = result == AVPlayerViewTrimResult::OKButton;
        let range = TRIM.with(|t| {
            t.borrow().as_ref().and_then(|(_, _, player)| unsafe {
                let item = player.currentItem()?;
                let start = item.reversePlaybackEndTime();
                let end = item.forwardPlaybackEndTime();
                Some((start, end, item.duration()))
            })
        });
        close_trim();
        if !ok {
            return;
        }
        let Some((start, end, dur)) = range else { return };
        let s = if unsafe { start.seconds() }.is_finite() { start } else { unsafe { kCMTimeZero } };
        let e = if unsafe { end.seconds() }.is_finite() { end } else { dur };
        let duration = unsafe { CMTime::with_seconds(e.seconds() - s.seconds(), 600) };
        apply_trim(path, CMTimeRange { start: s, duration });
    });
    unsafe { view.beginTrimmingWithCompletionHandler(Some(&block)) };
}

pub fn close_trim() {
    if let Some((w, _v, player)) = TRIM.with(|t| t.borrow_mut().take()) {
        unsafe { player.pause() };
        w.orderOut(None);
        w.close();
        ui::app(MainThreadMarker::new().unwrap()).deactivate();
    }
}

fn apply_trim(path: PathBuf, range: CMTimeRange) {
    let a = asset(&path);
    let out = path.with_extension("trimming.mp4");
    let _ = std::fs::remove_file(&out);
    let original = path.clone();
    passthrough(&a, Some(range), out, move |r| match r {
        Ok(tmp) => match std::fs::rename(&tmp, &original) {
            Ok(()) => finished_hud(original, false),
            Err(e) => hud::message("Couldn't replace the recording", Some(&e.to_string())),
        },
        Err(e) => hud::message("Couldn't trim the recording", Some(&e)),
    });
}

// ── GIF ────────────────────────────────────────────────────────────────────

const GIF_FPS: f64 = 12.0;
const GIF_MAX_SECONDS: f64 = 60.0;
const GIF_MAX_WIDTH: f64 = 960.0;

fn make_gif(video: PathBuf) {
    hud::toast("Making GIF…", None);
    let out = video.with_extension("gif");
    let job = MainBound((video, out));
    dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::QualityOfService(
        dispatch2::DispatchQoS::UserInitiated,
    ))
    .exec_async(move || {
        let whole = job;
        let (video, out) = whole.0;
        let result = write_gif(&video, &out);
        let r = MainBound(result.map(|()| out));
        util::main_async(move || {
            let whole = r;
            match whole.0 {
                Ok(path) => finished_hud(path, false),
                Err(e) => hud::message("Couldn't make the GIF", Some(&e)),
            }
        });
    });
}

fn cf_dict(keys: &[&CFString], vals: &[&AnyObject]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = keys.iter().map(|k| unsafe { &*(*k as *const CFString as *const NSString) }).collect();
    NSDictionary::from_slices(&keys, vals)
}

fn as_cf(d: &NSDictionary<NSString, AnyObject>) -> &CFDictionary {
    // SAFETY: NSDictionary is toll-free bridged with CFDictionary.
    unsafe { &*(d as *const NSDictionary<NSString, AnyObject> as *const CFDictionary) }
}

fn write_gif(video: &Path, out: &Path) -> Result<(), String> {
    let a = asset(video);
    let total = unsafe { a.duration().seconds() };
    if !total.is_finite() || total <= 0.0 {
        return Err("The recording has no frames.".into());
    }
    let secs = total.min(GIF_MAX_SECONDS);
    let frames = (secs * GIF_FPS).ceil().max(1.0) as usize;
    let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&a) };
    unsafe {
        generator.setAppliesPreferredTrackTransform(true);
        generator.setMaximumSize(CGSize::new(GIF_MAX_WIDTH, GIF_MAX_WIDTH));
        let tol = CMTime::with_seconds(0.5 / GIF_FPS, 600);
        generator.setRequestedTimeToleranceBefore(tol);
        generator.setRequestedTimeToleranceAfter(tol);
    }
    let url = file_url(out);
    let cf_url: &CFURL = unsafe { &*(Retained::as_ptr(&url) as *const CFURL) };
    let gif_type = NSString::from_str("com.compuserve.gif");
    let cf_type: &CFString = unsafe { &*(Retained::as_ptr(&gif_type) as *const CFString) };
    let dest: CFRetained<CGImageDestination> =
        unsafe { CGImageDestination::with_url(cf_url, cf_type, frames, None) }.ok_or("Couldn't create the GIF file.")?;

    let loop_props = cf_dict(&[unsafe { kCGImagePropertyGIFLoopCount }], &[&NSNumber::new_i32(0)]);
    let file_props = cf_dict(&[unsafe { kCGImagePropertyGIFDictionary }], &[&loop_props]);
    unsafe { dest.set_properties(Some(as_cf(&file_props))) };
    let delay = cf_dict(&[unsafe { kCGImagePropertyGIFDelayTime }], &[&NSNumber::new_f64(1.0 / GIF_FPS)]);
    let frame_props = cf_dict(&[unsafe { kCGImagePropertyGIFDictionary }], &[&delay]);

    for i in 0..frames {
        let t = unsafe { CMTime::with_seconds(i as f64 / GIF_FPS, 600) };
        #[allow(deprecated)]
        let img = unsafe { generator.copyCGImageAtTime_actualTime_error(t, std::ptr::null_mut()) };
        if let Ok(img) = img {
            unsafe { dest.add_image(&img, Some(as_cf(&frame_props))) };
        }
    }
    if unsafe { dest.finalize() } { Ok(()) } else { Err("Couldn't write the GIF.".into()) }
}

#[allow(unused)]
fn _unused(_: &NSError, _: &NSPanel) {}
