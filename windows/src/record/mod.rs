//! Screen recording with Windows.Graphics.Capture and a hardware H.264/HEVC
//! encoder (through `windows-capture`). Regions are cropped on the GPU, a picked
//! window is captured on its own, and pause simply stops feeding frames and audio
//! so the result is one continuous file.

mod audio;
mod clicks;
mod mf;
pub mod post;
mod webcam;

pub use audio::microphones;
pub use mf::cameras;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings as CaptureSettings,
};
use windows_capture::window::Window;

use crate::destination::{self, Destination, Folder, NameParts};
use crate::geometry::Rect;
use crate::hud::{self, Button, Content};
use crate::i18n::{tr, trf};
use crate::naming::Kind;
use crate::session::RecordTarget;
use crate::settings::{self, Codec, Settings};
use crate::toolbar::Icon;
use crate::commands::Command;
use crate::util;
use crate::app;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Countdown,
    Recording,
    Paused,
}

/// The recording clock: wall time minus pauses. Shared with capture/audio threads.
pub struct Clock {
    start: Mutex<Option<Instant>>,
    paused_total: Mutex<Duration>,
    paused_at: Mutex<Option<Instant>>,
    pub paused: AtomicBool,
}

impl Clock {
    fn new() -> Clock {
        Clock { start: Mutex::new(None), paused_total: Mutex::new(Duration::ZERO), paused_at: Mutex::new(None), paused: AtomicBool::new(false) }
    }
    fn begin(&self) {
        *self.start.lock().unwrap() = Some(Instant::now());
    }
    /// Recorded time so far (excludes pauses).
    pub fn elapsed(&self) -> Duration {
        let Some(start) = *self.start.lock().unwrap() else { return Duration::ZERO };
        let paused = *self.paused_total.lock().unwrap() + self.paused_at.lock().unwrap().map(|p| p.elapsed()).unwrap_or_default();
        start.elapsed().saturating_sub(paused)
    }
    fn pause(&self) {
        *self.paused_at.lock().unwrap() = Some(Instant::now());
        self.paused.store(true, Ordering::SeqCst);
    }
    fn resume(&self) {
        if let Some(p) = self.paused_at.lock().unwrap().take() {
            *self.paused_total.lock().unwrap() += p.elapsed();
        }
        self.paused.store(false, Ordering::SeqCst);
    }
}

/// Everything the capture thread needs.
pub struct Flags {
    clock: Arc<Clock>,
    path: PathBuf,
    /// Crop rect within the captured surface (x0, y0, x1, y1), for regions.
    crop: Option<(u32, u32, u32, u32)>,
    codec: Codec,
    fps: u32,
    audio: bool,
    frames: Arc<AtomicU64>,
    failed: Arc<Mutex<Option<String>>>,
}

pub struct Handler {
    encoder: Option<VideoEncoder>,
    size: (u32, u32),
    flags: Flags,
    last_ts: AtomicI64,
    scratch: Vec<u8>,
    padded: Vec<u8>,
}

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

impl Handler {
    fn ensure_encoder(&mut self, w: u32, h: u32) -> Result<(), String> {
        if self.encoder.is_some() {
            return Ok(());
        }
        let (w, h) = (even(w), even(h));
        let f = &self.flags;
        let bitrate = ((w as f64 * h as f64 * f.fps as f64 * 0.1) as u32).clamp(4_000_000, 40_000_000);
        let video = VideoSettingsBuilder::new(w, h)
            .sub_type(if f.codec == Codec::Hevc { VideoSettingsSubType::HEVC } else { VideoSettingsSubType::H264 })
            .frame_rate(f.fps)
            .bitrate(bitrate);
        let audio = AudioSettingsBuilder::new().disabled(!f.audio);
        let enc = VideoEncoder::new(video, audio, ContainerSettingsBuilder::default(), &f.path).map_err(|e| e.to_string())?;
        self.encoder = Some(enc);
        self.size = (w, h);
        Ok(())
    }
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Handler { encoder: None, size: (0, 0), flags: ctx.flags, last_ts: AtomicI64::new(-1), scratch: Vec::new(), padded: Vec::new() })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, _control: InternalCaptureControl) -> Result<(), Self::Error> {
        if self.flags.clock.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        // 100 ns ticks on the recording clock; drop frames above the target rate.
        let ts = (self.flags.clock.elapsed().as_nanos() / 100) as i64;
        let min_gap = 10_000_000 / self.flags.fps as i64;
        let last = self.last_ts.load(Ordering::Relaxed);
        if last >= 0 && ts - last < min_gap * 8 / 10 {
            return Ok(());
        }
        let (fw, fh) = (frame.width(), frame.height());
        let (x0, y0, x1, y1) = match self.flags.crop {
            Some((x0, y0, x1, y1)) => (x0.min(fw - 1), y0.min(fh - 1), x1.min(fw), y1.min(fh)),
            None => (0, 0, fw, fh),
        };
        if let Err(e) = self.ensure_encoder(x1 - x0, y1 - y0) {
            *self.flags.failed.lock().unwrap() = Some(e.clone());
            return Err(e);
        }
        let (ow, oh) = self.size;
        let fb = frame.buffer_crop(x0, y0, x1, y1).map_err(|e| e.to_string())?;
        let (bw, bh) = (fb.width(), fb.height());
        let bytes = fb.as_nopadding_buffer(&mut self.scratch);
        // The encoder takes bottom-up rows (DIB order), so flip while copying. This
        // also pads or crops a window that changed size into the fixed video size.
        self.padded.clear();
        self.padded.resize((ow * oh * 4) as usize, 0);
        let rows = bh.min(oh) as usize;
        let cols = (bw.min(ow) * 4) as usize;
        for r in 0..rows {
            let s = r * (bw * 4) as usize;
            let d = (oh as usize - 1 - r) * (ow * 4) as usize;
            self.padded[d..d + cols].copy_from_slice(&bytes[s..s + cols]);
        }
        let data: &[u8] = &self.padded;
        if let Some(enc) = &mut self.encoder {
            enc.send_frame_buffer(data, ts).map_err(|e| e.to_string())?;
            self.last_ts.store(ts, Ordering::Relaxed);
            self.flags.frames.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The window being recorded closed: stop and keep what we have.
        util::main_async(|| stop(false));
        Ok(())
    }
}

type Control = CaptureControl<Handler, String>;

struct Recording {
    target: RecordTarget,
    settings: Settings,
    state: State,
    clock: Arc<Clock>,
    control: Option<Control>,
    audio: Option<audio::Mixer>,
    frames: Arc<AtomicU64>,
    failed: Arc<Mutex<Option<String>>>,
    path: PathBuf,
    outline: Option<u64>,
    pill: Option<u64>,
    countdown: Option<u64>,
    webcam: Option<webcam::Bubble>,
    clicks: Option<clicks::Rings>,
    cancelled: bool,
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

pub fn elapsed() -> Duration {
    with(|r| r.clock.elapsed()).unwrap_or_default()
}

fn scratch_path() -> PathBuf {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
    destination::scratch_dir().join(format!("recording-{stamp}.mp4"))
}

/// Begin recording `target` (after the optional countdown).
pub fn start(target: RecordTarget) {
    if is_active() {
        return;
    }
    let mut s = settings::get();
    let window_mode = target.window.is_some();
    if window_mode && s.rec_webcam {
        hud::message(&tr("Webcam bubble is off for window recordings"), Some(&tr("Select a region instead to include the camera.")));
        s.rec_webcam = false;
    }
    let outline = (!window_mode).then(|| show_outline(&target));
    let webcam = if s.rec_webcam { webcam::Bubble::show(target.global_rect, target.scale, s.rec_webcam_device.as_deref()) } else { None };
    let countdown = s.rec_countdown.then(|| show_countdown(&target));
    let rec = Recording {
        target,
        settings: s,
        state: State::Countdown,
        clock: Arc::new(Clock::new()),
        control: None,
        audio: None,
        frames: Arc::new(AtomicU64::new(0)),
        failed: Arc::new(Mutex::new(None)),
        path: scratch_path(),
        outline,
        pill: None,
        countdown,
        webcam,
        clicks: None,
        cancelled: false,
    };
    let has_countdown = rec.countdown.is_some();
    REC.with(|r| *r.borrow_mut() = Some(rec));
    app::set_recording_ui(true);
    crate::hotkey::set_escape(has_countdown);
    if has_countdown {
        countdown_tick(3);
    } else {
        util::main_after(0.15, begin_capture);
    }
}

fn show_outline(t: &RecordTarget) -> u64 {
    let pad = (4.0 * t.scale).round();
    let r = Rect::new(t.global_rect.x - pad, t.global_rect.y - pad, t.global_rect.w + pad * 2.0, t.global_rect.h + pad * 2.0);
    hud::show(r, t.scale, Content::Outline { recording: false }, Vec::new(), true, false)
}

fn show_countdown(t: &RecordTarget) -> u64 {
    let s = t.scale;
    let (w, h) = (120.0 * s, 140.0 * s);
    let c = t.global_rect.mid();
    let r = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
    let skip = Button { rect: Rect::new(18.0 * s, h - 46.0 * s, w - 36.0 * s, 30.0 * s), cmd: Command::SkipCountdown, icon: None, label: Some(tr("Skip")), tooltip: tr("Start recording now") };
    hud::show(r, s, Content::Countdown { n: 3 }, vec![skip], false, false)
}

fn countdown_tick(n: u32) {
    let id = with(|r| (r.state == State::Countdown).then_some(r.countdown).flatten()).flatten();
    let Some(id) = id else { return };
    if n == 0 {
        hud::close(id);
        with(|r| r.countdown = None);
        begin_capture();
    } else {
        hud::update(id, |p| p.content = Content::Countdown { n });
        util::main_after(1.0, move || countdown_tick(n - 1));
    }
}

/// Start recording now instead of waiting for the countdown.
pub fn skip_countdown() {
    let id = with(|r| if r.state == State::Countdown { r.countdown.take() } else { None }).flatten();
    if let Some(id) = id {
        hud::close(id);
        begin_capture();
    }
}

fn begin_capture() {
    crate::hotkey::set_escape(false);
    let started = with(|r| -> Result<(), String> {
        if r.state != State::Countdown {
            return Ok(());
        }
        let t = &r.target;
        let s = &r.settings;
        let audio_on = s.rec_system_audio || s.rec_mic;
        let crop = if t.window.is_some() {
            None
        } else {
            let reg = t.region;
            Some((reg.x.max(0.0) as u32, reg.y.max(0.0) as u32, (reg.x + reg.w) as u32, (reg.y + reg.h) as u32))
        };
        let make_flags = || Flags {
            clock: r.clock.clone(),
            path: r.path.clone(),
            crop,
            codec: s.rec_codec,
            fps: s.rec_fps,
            audio: audio_on,
            frames: r.frames.clone(),
            failed: r.failed.clone(),
        };
        let cursor = if s.rec_cursor { CursorCaptureSettings::WithCursor } else { CursorCaptureSettings::WithoutCursor };
        let interval = MinimumUpdateIntervalSettings::Custom(Duration::from_micros(1_000_000 / s.rec_fps as u64));
        r.clock.begin();
        // Prefer no yellow capture border and a capped update rate (both Windows 11);
        // fall back step by step on Windows 10. Frames are rate-limited here anyway.
        let mut last_err = String::new();
        let mut control = None;
        let options = [
            (DrawBorderSettings::WithoutBorder, interval),
            (DrawBorderSettings::Default, interval),
            (DrawBorderSettings::WithoutBorder, MinimumUpdateIntervalSettings::Default),
            (DrawBorderSettings::Default, MinimumUpdateIntervalSettings::Default),
        ];
        for (border, interval) in options {
            let res = match t.window {
                Some(hwnd) => {
                    let item = Window::from_raw_hwnd(hwnd as *mut _);
                    let cs = CaptureSettings::new(item, cursor, border, SecondaryWindowSettings::Default, interval, DirtyRegionSettings::Default, ColorFormat::Bgra8, make_flags());
                    Handler::start_free_threaded(cs).map_err(|e| format!("{e:?}"))
                }
                None => {
                    let item = Monitor::from_raw_hmonitor(t.monitor as *mut _);
                    let cs = CaptureSettings::new(item, cursor, border, SecondaryWindowSettings::Default, interval, DirtyRegionSettings::Default, ColorFormat::Bgra8, make_flags());
                    Handler::start_free_threaded(cs).map_err(|e| format!("{e:?}"))
                }
            };
            match res {
                Ok(c) => {
                    control = Some(c);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let control = control.ok_or(last_err)?;
        if audio_on {
            let callback = control.callback();
            r.audio = audio::Mixer::start(r.clock.clone(), s.rec_system_audio, s.rec_mic, s.rec_mic_device.clone(), move |pcm: &[u8]| {
                let mut h = callback.lock();
                if let Some(enc) = &mut h.encoder {
                    let _ = enc.send_audio_buffer(pcm, 0);
                    true
                } else {
                    false
                }
            });
        }
        r.control = Some(control);
        r.state = State::Recording;
        if s.rec_click_rings {
            r.clicks = clicks::Rings::start(r.target.scale);
        }
        Ok(())
    });
    match started {
        Some(Ok(())) => {
            with(|r| {
                if let Some(id) = r.outline {
                    hud::update(id, |p| p.content = Content::Outline { recording: true });
                }
            });
            show_pill();
            tick();
        }
        Some(Err(e)) => fail(&e),
        None => {}
    }
}

/// The floating control: red dot + clock, pause/resume and stop. Drag to move.
fn show_pill() {
    let Some(t) = with(|r| r.target.clone()) else { return };
    let s = t.scale;
    let (w, h) = (196.0 * s, 40.0 * s);
    // Above the region (or inside its top edge when there's no room), centred.
    let (_, work, _) = hud::monitor_at(t.global_rect.mid());
    let mut y = t.global_rect.y - h - 12.0 * s;
    if y < work.y + 4.0 * s {
        y = t.global_rect.y + 12.0 * s;
    }
    let r = Rect::new(t.global_rect.mid().x - w / 2.0, y, w, h).clamp_inside(&work.inset(4.0 * s));
    let b = 32.0 * s;
    let buttons = vec![
        Button { rect: Rect::new(w - b * 2.0 - 8.0 * s, 4.0 * s, b, b), cmd: Command::PauseResume, icon: Some(Icon::Pause), label: None, tooltip: tr("Pause") },
        Button { rect: Rect::new(w - b - 6.0 * s, 4.0 * s, b, b), cmd: Command::StopRecording, icon: Some(Icon::Stop), label: None, tooltip: tr("Stop and save") },
    ];
    let id = hud::show(r, s, Content::Pill { text: "00:00".into(), paused: false }, buttons, false, true);
    with(|r| r.pill = Some(id));
}

fn tick() {
    let info = with(|r| (r.pill, r.clock.elapsed(), r.state == State::Paused));
    let Some((pill, el, paused)) = info else { return };
    if let Some(id) = pill {
        let secs = el.as_secs();
        let text = if secs >= 3600 {
            format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
        } else {
            format!("{:02}:{:02}", secs / 60, secs % 60)
        };
        hud::update(id, |p| {
            p.content = Content::Pill { text: if paused { trf("Paused {time}", &[("time", &text)]) } else { text }, paused };
            if let Some(b) = p.buttons.first_mut() {
                b.icon = Some(if paused { Icon::Play } else { Icon::Pause });
                b.tooltip = if paused { tr("Resume") } else { tr("Pause") };
            }
        });
    }
    app::refresh_recording_ui();
    util::main_after(0.5, tick);
}

pub fn toggle_pause() {
    with(|r| match r.state {
        State::Recording => {
            r.clock.pause();
            r.state = State::Paused;
            if let Some(id) = r.outline {
                hud::update(id, |p| p.content = Content::Outline { recording: false });
            }
        }
        State::Paused => {
            r.clock.resume();
            r.state = State::Recording;
            if let Some(id) = r.outline {
                hud::update(id, |p| p.content = Content::Outline { recording: true });
            }
        }
        _ => {}
    });
    app::refresh_recording_ui();
}

/// Stop and save. `cancel` discards the recording instead.
pub fn stop(cancel: bool) {
    let Some(rec) = REC.with(|r| r.borrow_mut().take()) else { return };
    let Recording { target, settings: s, state, control, audio, frames, failed, path, outline, pill, countdown, webcam, clicks, mut cancelled, .. } = rec;
    cancelled |= cancel || state == State::Countdown;
    for id in [outline, pill, countdown].into_iter().flatten() {
        hud::close(id);
    }
    drop(webcam);
    drop(clicks);
    app::set_recording_ui(false);
    crate::hotkey::set_escape(false);
    if let Some(a) = audio {
        a.stop();
    }
    // Stop capture and finish the file off the UI thread (finishing flushes the encoder).
    let job = util::MainBound((control, path, target, s, cancelled, frames, failed));
    std::thread::spawn(move || {
        let whole = job;
        let (control, path, target, s, cancelled, frames, failed) = whole.0;
        let mut err = failed.lock().unwrap().clone();
        if let Some(control) = control {
            let callback = control.callback();
            let _ = control.stop();
            let enc = callback.lock().encoder.take();
            if let Some(enc) = enc
                && let Err(e) = enc.finish()
            {
                err.get_or_insert(e.to_string());
            }
        }
        let have_file = path.exists() && frames.load(Ordering::Relaxed) > 0;
        let payload = util::MainBound((path, target, s, cancelled, have_file, err));
        util::main_async(move || {
            let whole = payload;
            let (path, target, s, cancelled, have_file, err) = whole.0;
            if cancelled || !have_file {
                let _ = std::fs::remove_file(&path);
                if !cancelled {
                    let detail = err.unwrap_or_else(|| tr("No frames were captured."));
                    hud::message(&tr("Couldn't save the recording"), Some(&detail));
                }
            } else {
                deliver(path, &target, &s);
            }
            util::release_memory_soon();
        });
    });
}

fn fail(msg: &str) {
    util::log(format!("recording failed: {msg}"));
    hud::message(&tr("Recording stopped"), Some(msg));
    stop(true);
}

fn deliver(file: PathBuf, target: &RecordTarget, s: &Settings) {
    let resolved = destination::resolve(&s.rec_dir, &Settings::default_rec_dir());
    let _ = std::fs::create_dir_all(&resolved.dir);
    let parts = NameParts {
        kind: Kind::Recording,
        app: target.front_app.clone(),
        size: Some((target.region.w as u32, target.region.h as u32)),
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
        Err(e) => hud::message(&tr("Couldn't save the recording"), Some(&e)),
    }
}
