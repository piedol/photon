//! Screen recording with ScreenCaptureKit's recording output (hardware encoded,
//! written straight to disk). Pause/resume records separate segments that are
//! joined without re-encoding when the recording stops.

mod indicator;
pub mod post;
mod webcam;

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, Message};
use objc2_av_foundation::{AVFileTypeMPEG4, AVVideoCodecTypeH264, AVVideoCodecTypeHEVC};
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_screen_capture_kit::{
    SCContentFilter, SCRecordingOutput, SCRecordingOutputConfiguration, SCRecordingOutputDelegate,
    SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCWindow,
    SCCaptureResolutionType,
};

use crate::output::destination::{self, Destination, Folder, NameParts};
use crate::output::naming::Kind;
use crate::session::RecordTarget;
use crate::settings::{self, Codec, Settings};
use crate::util::{self, ns, MainBound};
use crate::{app, hud, permissions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Countdown,
    Starting,
    Recording,
    Paused,
    Stopping,
}

struct Recording {
    target: RecordTarget,
    settings: Settings,
    state: State,
    stream: Option<Retained<SCStream>>,
    output: Option<Retained<SCRecordingOutput>>,
    delegate: Retained<RecDelegate>,
    segments: Vec<PathBuf>,
    /// Output identity (pointer) per segment, and whether its file is complete.
    segment_ids: Vec<usize>,
    done: Vec<bool>,
    stream_stopped: bool,
    cancelled: bool,
    started: Option<Instant>,
    paused_total: Duration,
    paused_at: Option<Instant>,
    /// Region outline (none when recording a single window, which can move).
    indicator: Option<indicator::Outline>,
    webcam: Option<webcam::Bubble>,
    countdown: Option<indicator::Countdown>,
}

thread_local! {
    static REC: RefCell<Option<Recording>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Recording) -> R) -> Option<R> {
    REC.with(|r| r.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn is_active() -> bool {
    REC.with(|r| r.borrow().is_some())
}

pub fn is_counting_down() -> bool {
    with(|r| r.state == State::Countdown).unwrap_or(false)
}

pub fn is_paused() -> bool {
    with(|r| r.state == State::Paused).unwrap_or(false)
}

/// Elapsed recording time (excludes pauses).
pub fn elapsed() -> Duration {
    with(|r| {
        let Some(start) = r.started else { return Duration::ZERO };
        let paused = r.paused_total + r.paused_at.map(|p| p.elapsed()).unwrap_or_default();
        start.elapsed().saturating_sub(paused)
    })
    .unwrap_or_default()
}

define_class!(
    // SAFETY: NSObject subclass, callbacks only hop to the main queue; no Drop impl.
    #[unsafe(super = NSObject)]
    #[name = "PhotonRecordingDelegate"]
    struct RecDelegate;

    unsafe impl NSObjectProtocol for RecDelegate {}

    unsafe impl SCRecordingOutputDelegate for RecDelegate {
        #[unsafe(method(recordingOutputDidStartRecording:))]
        fn did_start(&self, _o: &SCRecordingOutput) {
            util::log("segment started");
        }

        #[unsafe(method(recordingOutput:didFailWithError:))]
        fn did_fail(&self, _o: &SCRecordingOutput, e: &NSError) {
            let msg = e.localizedDescription().to_string();
            util::main_async(move || fail(&msg));
        }

        #[unsafe(method(recordingOutputDidFinishRecording:))]
        fn did_finish(&self, o: &SCRecordingOutput) {
            util::log("segment finished");
            let id = o as *const SCRecordingOutput as usize;
            util::main_async(move || mark_done(Some(id)));
        }
    }

    unsafe impl SCStreamDelegate for RecDelegate {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _s: &SCStream, e: &NSError) {
            let msg = e.localizedDescription().to_string();
            util::main_async(move || fail(&msg));
        }
    }
);

impl RecDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Begin recording `target` (after the optional countdown).
pub fn start(target: RecordTarget) {
    if is_active() {
        return;
    }
    let mtm = app::mtm();
    let mut s = settings::get();
    if s.rec_mic && !permissions::authorized(false) {
        permissions::ask_microphone();
        s.rec_mic = false;
    }
    if s.rec_webcam && !permissions::authorized(true) {
        permissions::ask_camera();
        s.rec_webcam = false;
    }
    let window_mode = target.window_id.is_some();
    let indicator = (!window_mode).then(|| indicator::Outline::show(mtm, target.global_rect));
    if window_mode && s.rec_webcam {
        hud::message("Webcam bubble is off for window recordings", Some("Select a region instead to include the camera."));
        s.rec_webcam = false;
    }
    let webcam = if s.rec_webcam { webcam::Bubble::show(mtm, target.global_rect, s.rec_webcam_device.as_deref()) } else { None };
    let countdown = s.rec_countdown.then(|| indicator::Countdown::show(mtm, target.global_rect));
    let countdown_on = countdown.is_some();
    let rec = Recording {
        target,
        settings: s,
        state: State::Countdown,
        stream: None,
        output: None,
        delegate: RecDelegate::new(),
        segments: Vec::new(),
        segment_ids: Vec::new(),
        done: Vec::new(),
        stream_stopped: false,
        cancelled: false,
        started: None,
        paused_total: Duration::ZERO,
        paused_at: None,
        indicator,
        webcam,
        countdown,
    };
    REC.with(|r| *r.borrow_mut() = Some(rec));
    app::set_recording_ui(true);
    // Esc cancels during the countdown.
    crate::hotkey::set_escape(countdown_on);
    if with(|r| r.countdown.is_some()).unwrap_or(false) {
        countdown_tick(3);
    } else {
        // Give the webcam bubble a moment to appear in the window list.
        util::main_after(0.15, begin_capture);
    }
}

/// Start recording now instead of waiting for the countdown.
pub fn skip_countdown() {
    let skipped = with(|r| {
        if r.state != State::Countdown {
            return false;
        }
        r.countdown.take();
        true
    })
    .unwrap_or(false);
    if skipped {
        begin_capture();
    }
}

fn countdown_tick(n: u32) {
    let alive = with(|r| {
        if r.state != State::Countdown {
            return false;
        }
        if let Some(c) = &r.countdown {
            c.set(n);
        }
        true
    })
    .unwrap_or(false);
    if !alive {
        return;
    }
    if n == 0 {
        with(|r| r.countdown.take());
        begin_capture();
    } else {
        util::main_after(1.0, move || countdown_tick(n - 1));
    }
}

fn even(v: f64) -> usize {
    let v = v.round() as usize;
    (v + 1) & !1
}

fn new_segment_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    destination::scratch_dir().join(format!("segment-{stamp}.mp4"))
}

fn make_output(r: &Recording) -> (Retained<SCRecordingOutput>, PathBuf) {
    let path = new_segment_path();
    let cfg = unsafe { SCRecordingOutputConfiguration::new() };
    unsafe {
        cfg.setOutputURL(&NSURL::fileURLWithPath(&ns(&path.to_string_lossy())));
        if let Some(t) = AVFileTypeMPEG4 {
            cfg.setOutputFileType(t);
        }
        let codec = if r.settings.rec_codec == Codec::Hevc { AVVideoCodecTypeHEVC } else { AVVideoCodecTypeH264 };
        if let Some(c) = codec {
            cfg.setVideoCodecType(c);
        }
    }
    let delegate = ProtocolObject::from_ref(&*r.delegate);
    let out = unsafe {
        SCRecordingOutput::initWithConfiguration_delegate(SCRecordingOutput::alloc(), &cfg, delegate)
    };
    (out, path)
}

fn begin_capture() {
    with(|r| r.state = State::Starting);
    crate::hotkey::set_escape(false);
    let bubble_id = with(|r| r.webcam.as_ref().map(|w| w.window_number())).flatten();
    let handler = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let Some(content) = (unsafe { content.as_ref() }) else {
            let msg = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
            let msg = msg.unwrap_or_else(|| "Screen capture isn't available.".into());
            util::main_async(move || fail(&msg));
            return;
        };
        let content = MainBound(content.retain());
        util::main_async(move || {
            let whole = content;
            configure_and_start(&whole.0, bubble_id);
        });
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &handler);
    }
}

fn configure_and_start(content: &SCShareableContent, bubble_id: Option<u32>) {
    let result = with(|r| -> Result<Retained<SCStream>, String> {
        let displays = unsafe { content.displays() };
        let display = displays
            .iter()
            .find(|d| unsafe { d.displayID() } == r.target.display_id)
            .ok_or("The display is no longer available.")?;
        let me = std::process::id() as i32;
        let apps: Vec<_> = unsafe { content.applications() }.iter().filter(|a| unsafe { a.processID() } == me).collect();
        let keep: Vec<Retained<SCWindow>> = unsafe { content.windows() }
            .iter()
            .filter(|w| Some(unsafe { w.windowID() }) == bubble_id)
            .collect();
        let t = &r.target;
        let s = &r.settings;
        // A clicked window is recorded on its own: it follows the window and
        // never shows what's around or in front of it.
        let window = t.window_id.and_then(|id| unsafe { content.windows() }.iter().find(|w| unsafe { w.windowID() } == id));
        let cfg = unsafe { SCStreamConfiguration::new() };
        let filter = match &window {
            Some(w) => unsafe {
                let f = w.frame();
                cfg.setWidth(even(f.size.width * t.scale));
                cfg.setHeight(even(f.size.height * t.scale));
                cfg.setIgnoreShadowsSingleWindow(true);
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), w)
            },
            None => unsafe {
                cfg.setSourceRect(crate::capture::cg_rect(t.source_rect));
                cfg.setWidth(even(t.source_rect.w * t.scale));
                cfg.setHeight(even(t.source_rect.h * t.scale));
                SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &NSArray::from_retained_slice(&apps),
                    &NSArray::from_retained_slice(&keep),
                )
            },
        };
        unsafe {
            cfg.setMinimumFrameInterval(CMTime::new(1, s.rec_fps as i32));
            cfg.setShowsCursor(s.rec_cursor);
            cfg.setShowMouseClicks(s.rec_click_rings);
            cfg.setCaptureResolution(SCCaptureResolutionType::Best);
            cfg.setQueueDepth(6);
            cfg.setCapturesAudio(s.rec_system_audio);
            cfg.setExcludesCurrentProcessAudio(true);
            cfg.setSampleRate(48_000);
            cfg.setChannelCount(2);
            cfg.setCaptureMicrophone(s.rec_mic);
            if let Some(id) = &s.rec_mic_device {
                cfg.setMicrophoneCaptureDeviceID(Some(&NSString::from_str(id)));
            }
        }
        let delegate = ProtocolObject::from_ref(&*r.delegate);
        let stream = unsafe { SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &cfg, Some(delegate)) };
        let (output, path) = make_output(r);
        unsafe { stream.addRecordingOutput_error(&output) }.map_err(|e| e.localizedDescription().to_string())?;
        r.segments.push(path);
        r.segment_ids.push(Retained::as_ptr(&output) as usize);
        r.done.push(false);
        r.output = Some(output);
        r.stream = Some(stream.clone());
        Ok(stream)
    });
    let stream = match result {
        Some(Ok(s)) => s,
        Some(Err(e)) => return fail(&e),
        None => return,
    };
    let done = RcBlock::new(move |error: *mut NSError| {
        let err = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
        util::main_async(move || match err {
            Some(e) => fail(&e),
            None => {
                with(|r| {
                    r.state = State::Recording;
                    r.started = Some(Instant::now());
                    if let Some(i) = &r.indicator {
                        i.set_recording(true);
                    }
                });
                app::refresh_recording_ui();
            }
        });
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&done)) };
}

/// Pause (finishes the current segment) or resume (starts a new one).
pub fn toggle_pause() {
    with(|r| match r.state {
        State::Recording => {
            if let (Some(stream), Some(out)) = (&r.stream, r.output.take()) {
                if let Err(e) = unsafe { stream.removeRecordingOutput_error(&out) } {
                    util::log(format!("pause failed: {e:?}"));
                }
                // Removing an output finalises its file but may not call the delegate.
                let id = Retained::as_ptr(&out) as usize;
                util::main_after(0.8, move || mark_done(Some(id)));
            }
            r.state = State::Paused;
            r.paused_at = Some(Instant::now());
            if let Some(i) = &r.indicator {
                i.set_recording(false);
            }
        }
        State::Paused => {
            let (out, path) = make_output(r);
            if let Some(stream) = &r.stream {
                match unsafe { stream.addRecordingOutput_error(&out) } {
                    Ok(()) => {
                        r.segments.push(path);
                        r.segment_ids.push(Retained::as_ptr(&out) as usize);
                        r.done.push(false);
                        r.output = Some(out);
                    }
                    Err(e) => util::log(format!("resume failed: {e:?}")),
                }
            }
            if let Some(p) = r.paused_at.take() {
                r.paused_total += p.elapsed();
            }
            r.state = State::Recording;
            if let Some(i) = &r.indicator {
                i.set_recording(true);
            }
        }
        _ => {}
    });
    app::refresh_recording_ui();
}

/// Stop and save. `cancel` discards the recording instead.
pub fn stop(cancel: bool) {
    let stream = with(|r| {
        r.cancelled |= cancel;
        if r.state == State::Stopping {
            return None;
        }
        let was = r.state;
        r.state = State::Stopping;
        if let Some(p) = r.paused_at.take() {
            r.paused_total += p.elapsed();
        }
        if matches!(was, State::Countdown) {
            r.cancelled = true;
            r.stream_stopped = true;
            return None;
        }
        r.stream.clone()
    })
    .flatten();
    match stream {
        Some(stream) => {
            let done = RcBlock::new(move |_e: *mut NSError| {
                util::log("stream stopped");
                util::main_async(|| {
                    with(|r| r.stream_stopped = true);
                    maybe_finalize();
                });
            });
            unsafe { stream.stopCaptureWithCompletionHandler(Some(&done)) };
            // Safety net: never leave a recording hanging if a callback is lost.
            util::main_after(6.0, || {
                with(|r| r.stream_stopped = true);
                mark_done(None);
            });
        }
        None => maybe_finalize(),
    }
}

/// Mark one segment's file complete (or all of them with `None`), then try to finish.
fn mark_done(id: Option<usize>) {
    with(|r| match id {
        Some(id) => {
            if let Some(i) = r.segment_ids.iter().position(|x| *x == id) {
                r.done[i] = true;
            }
        }
        None => r.done.iter_mut().for_each(|d| *d = true),
    });
    maybe_finalize();
}

fn fail(msg: &str) {
    util::log(format!("recording failed: {msg}"));
    // Keep whatever was recorded; segments that never finalised are dropped by `join`.
    let had = with(|r| {
        r.state = State::Stopping;
        r.stream_stopped = true;
    });
    if had.is_some() {
        hud::message("Recording stopped", Some(msg));
        mark_done(None);
    }
}

/// Once the stream has stopped and every segment file is complete, tear down
/// the indicators and produce the final file.
fn maybe_finalize() {
    let ready = with(|r| r.state == State::Stopping && r.stream_stopped && r.done.iter().all(|d| *d)).unwrap_or(false);
    if !ready {
        return;
    }
    let Some(rec) = REC.with(|r| r.borrow_mut().take()) else { return };
    if !crate::session::is_open() {
        crate::hotkey::set_escape(false);
    }
    util::log(format!("finalizing {} segment(s)", rec.segments.len()));
    let Recording { target, settings, segments, cancelled, indicator, webcam, .. } = rec;
    drop(indicator);
    drop(webcam);
    app::set_recording_ui(false);
    util::release_memory_soon();
    let segments: Vec<PathBuf> = segments.into_iter().filter(|p| p.exists()).collect();
    if cancelled || segments.is_empty() {
        for p in segments {
            let _ = std::fs::remove_file(p);
        }
        return;
    }
    post::join(segments, move |joined| match joined {
        Ok(file) => deliver(file, &target, &settings),
        Err(e) => hud::message("Couldn't save the recording", Some(&e)),
    });
}

fn deliver(file: PathBuf, target: &RecordTarget, s: &Settings) {
    let resolved = destination::resolve(&s.rec_dir, &Settings::default_rec_dir());
    let _ = std::fs::create_dir_all(&resolved.dir);
    let parts = NameParts {
        kind: Kind::Recording,
        app: target.front_app.clone(),
        size: Some(target.source_rect.pixel_size(target.scale)),
        display: Some(target.display_name.clone()),
    };
    let (path, counter) = destination::unique_path(&resolved.dir, &s.rec_naming, s.rec_counter, &parts, "mp4");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match Folder(resolved.dir.clone()).deliver(&file, &name) {
        Ok(saved) => {
            util::log(format!("saved {}", saved.display()));
            settings::update(|st| st.rec_counter = counter + 1);
            if s.rec_also_copy
                && let Some(also) = s.rec_also_dir.as_ref().filter(|d| destination::is_usable(d)) {
                    let _ = std::fs::create_dir_all(also);
                    let _ = std::fs::copy(&saved, also.join(&name));
                }
            post::finished_hud(saved, resolved.fell_back);
        }
        Err(e) => hud::message("Couldn't save the recording", Some(&e)),
    }
}
