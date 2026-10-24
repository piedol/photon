//! Developer self-tests (`photon://selftest…` when started with PHOTON_DEV=1).
//! They drive real sessions and save screenshots to `C:\Photon\shots`.

use crate::commands::Command;
use crate::geometry::{pt, Rect};
use crate::overlay::MouseKind;
use crate::session;
use crate::toolbar::Mode;
use crate::util;

fn shot(name: &str) {
    let dir = std::env::var("PHOTON_SHOTS").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("photon-shots"));
    let _ = std::fs::create_dir_all(&dir);
    let ds = crate::capture::displays();
    let Some(d) = ds.first() else { return };
    let f = d.frame;
    if let Some(px) = crate::capture::grab(f) {
        let img = crate::export::Image { pixels: px, width: f.w as u32, height: f.h as u32, dpi: 96.0 };
        if let Some(png) = crate::export::encode(&img, false) {
            let _ = std::fs::write(dir.join(format!("{name}.png")), png);
        }
    }
    util::log(format!("selftest shot {name}"));
}

fn mouse(kind: MouseKind, x: f64, y: f64) {
    session::mouse(0, kind, pt(x, y));
}

fn drag(a: (f64, f64), b: (f64, f64)) {
    mouse(MouseKind::Down, a.0, a.1);
    for i in 1..=12 {
        let t = i as f64 / 12.0;
        mouse(MouseKind::Drag, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    }
    mouse(MouseKind::Up, b.0, b.1);
}

fn tool(i: u8) {
    session::command(Command::Tool(i));
}

type Step = Box<dyn FnOnce()>;

fn run_steps(mut steps: Vec<(f64, Step)>) {
    if steps.is_empty() {
        util::log("selftest done");
        return;
    }
    let (delay, f) = steps.remove(0);
    util::main_after(delay.max(0.01), move || {
        f();
        run_steps(steps);
    });
}

pub fn run(which: &str) {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    // Coordinates are physical pixels on the primary display.
    let d = crate::capture::displays().first().map(|d| d.frame).unwrap_or(Rect::new(0.0, 0.0, 1920.0, 1080.0));
    let (w, h) = (d.w, d.h);
    let (x0, y0) = (w * 0.18, h * 0.22);
    let (x1, y1) = (w * 0.62, h * 0.70);
    let steps: Vec<(f64, Step)> = match which {
        "selftest" => vec![
            (0.0, Box::new(|| session::start(Mode::Screenshot))),
            (0.8, Box::new(move || mouse(MouseKind::Move, w * 0.5, h * 0.5))),
            (0.3, Box::new(|| shot("01-hover"))),
            (0.1, Box::new(move || drag((x0, y0), (x1, y1)))),
            (0.3, Box::new(|| shot("02-selected"))),
            (0.1, Box::new(move || {
                let (cx, cy) = (x0 + 40.0, y0 + 60.0);
                tool(2);
                drag((cx, cy), (cx + 220.0, cy + 120.0));
                tool(3);
                drag((cx + 260.0, cy), (cx + 420.0, cy + 100.0));
                tool(4);
                drag((cx + 460.0, cy), (cx + 560.0, cy + 100.0));
                tool(0);
                drag((cx, cy + 180.0), (cx + 160.0, cy + 240.0));
                tool(5);
                drag((cx + 200.0, cy + 200.0), (cx + 420.0, cy + 200.0));
                tool(7);
                drag((cx + 440.0, cy + 170.0), (cx + 640.0, cy + 240.0));
                tool(8);
                for k in 0..3 {
                    let x = cx + 20.0 + k as f64 * 50.0;
                    mouse(MouseKind::Down, x, cy + 300.0);
                    mouse(MouseKind::Up, x, cy + 300.0);
                }
                tool(1);
                drag((cx + 200.0, cy + 300.0), (cx + 380.0, cy + 320.0));
                tool(6);
                mouse(MouseKind::Down, cx + 420.0, cy + 300.0);
                mouse(MouseKind::Up, cx + 420.0, cy + 300.0);
                for c in "Photon text".chars() {
                    session::char_input(c as u32);
                }
            })),
            (0.4, Box::new(|| shot("03-markup"))),
            (0.1, Box::new(|| {
                session::key_down(crate::keys::vk::RETURN);
                session::command(Command::ToggleSwatches);
            })),
            (0.3, Box::new(|| shot("04-swatches"))),
            (0.1, Box::new(|| {
                session::command(Command::ToggleSwatches);
                session::command(Command::Copy);
                clipboard_report();
            })),
            (0.3, Box::new(|| shot("05-copied"))),
            (1.5, Box::new(|| session::start(Mode::Record))),
            (0.8, Box::new(move || mouse(MouseKind::Move, w * 0.5, h * 0.5))),
            (0.2, Box::new(move || mouse(MouseKind::Down, w * 0.5, h * 0.5))),
            (0.05, Box::new(move || mouse(MouseKind::Up, w * 0.5, h * 0.5))),
            (0.3, Box::new(|| shot("06-record-window-picked"))),
            (0.1, Box::new(|| session::command(Command::Close))),
        ],
        "selftest-record" => vec![
            (0.0, Box::new(|| session::start(Mode::Record))),
            (0.8, Box::new(move || drag((x0, y0), (x1, y1)))),
            (0.3, Box::new(|| shot("10-record-selected"))),
            (0.1, Box::new(|| session::command(Command::StartRecording))),
            (1.2, Box::new(|| shot("11-countdown"))),
            (0.1, Box::new(|| crate::commands::run(Command::SkipCountdown))),
            (3.0, Box::new(|| shot("12-recording"))),
            (0.1, Box::new(crate::record::toggle_pause)),
            (1.0, Box::new(|| shot("13-paused"))),
            (0.1, Box::new(crate::record::toggle_pause)),
            (2.0, Box::new(|| crate::commands::run(Command::CaptureScreenshot))),
            (6.0, Box::new(|| shot("14-saved"))),
            (0.2, Box::new(|| crate::record::post::command_for_test(Command::ExportGif))),
            (10.0, Box::new(|| shot("16-gif"))),
            (0.2, Box::new(|| crate::record::post::command(Command::DismissHud))),
            (0.2, Box::new(crate::record::post::trim_last_video_for_test)),
            (3.0, Box::new(|| shot("15-trim"))),
            (0.2, Box::new(crate::record::post::close_trim_for_test)),
        ],
        "selftest-settings" => vec![
            (0.0, Box::new(crate::settings_window::show)),
            (0.8, Box::new(|| shot("20-general"))),
            (0.1, Box::new(|| crate::settings_window::select_tab_for_test(1))),
            (0.5, Box::new(|| shot("21-screenshots"))),
            (0.1, Box::new(|| crate::settings_window::select_tab_for_test(2))),
            (0.5, Box::new(|| shot("22-recordings"))),
            (0.1, Box::new(crate::settings_window::close_for_test)),
        ],
        "selftest-trim" => vec![
            (0.0, Box::new(crate::record::post::trim_last_video_for_test)),
            (3.0, Box::new(|| shot("15-trim"))),
            (0.2, Box::new(crate::record::post::close_trim_for_test)),
        ],
        "selftest-trim-apply" => vec![(0.0, Box::new(|| {
            let dir = crate::settings::get().rec_dir;
            let newest = std::fs::read_dir(&dir).ok().and_then(|rd| {
                rd.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "mp4"))
                    .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                    .map(|e| e.path())
            });
            if let Some(p) = newest {
                util::log(format!("trimming {}", p.display()));
                crate::record::post::apply_trim(p, 0.5, 2.5);
            }
        }))],
        "selftest-gif" => vec![
            (0.0, Box::new(crate::record::post::gif_last_video_for_test)),
            (10.0, Box::new(|| shot("16-gif"))),
        ],
        "selftest-window" => vec![
            (0.0, Box::new(|| session::start(Mode::Record))),
            (0.8, Box::new(move || mouse(MouseKind::Move, w * 0.3, h * 0.3))),
            (0.1, Box::new(move || mouse(MouseKind::Down, w * 0.3, h * 0.3))),
            (0.05, Box::new(move || mouse(MouseKind::Up, w * 0.3, h * 0.3))),
            (0.3, Box::new(|| shot("30-window-picked"))),
            (0.1, Box::new(|| session::command(Command::StartRecording))),
            (0.3, Box::new(|| crate::commands::run(Command::SkipCountdown))),
            (3.0, Box::new(|| shot("31-window-recording"))),
            (0.1, Box::new(|| crate::commands::run(Command::RecordHotkey))),
            (4.0, Box::new(|| shot("32-window-saved"))),
        ],
        "selftest-misc" => vec![
            (0.0, Box::new(|| {
                util::log(format!("mics: {:?}", crate::record::microphones().iter().map(|m| &m.1).collect::<Vec<_>>()));
                util::log(format!("cameras: {:?}", crate::record::cameras().iter().map(|m| &m.1).collect::<Vec<_>>()));
                util::log(format!("cloud: {:?}", crate::cloud::discover().iter().map(|f| f.label.clone()).collect::<Vec<_>>()));
                util::log(format!("defaults: shots {:?} recordings {:?}", crate::settings::Settings::default_shot_dir(), crate::settings::Settings::default_rec_dir()));
                crate::settings::update(|s| s.shot_autosave = true);
                session::start(Mode::Screenshot);
            })),
            (0.8, Box::new(move || drag((x0, y0), (x0 + 500.0, y0 + 300.0)))),
            (0.2, Box::new(|| session::command(Command::Copy))),
            (1.0, Box::new(|| {
                crate::settings::update(|s| s.shot_autosave = false);
                let dir = crate::settings::get().shot_dir;
                let n = std::fs::read_dir(&dir).map(|r| r.count()).unwrap_or(0);
                util::log(format!("autosave dir {} has {n} file(s)", dir.display()));
                session::start(Mode::Screenshot);
            })),
            (0.8, Box::new(move || drag((x0, y0), (x0 + 500.0, y0 + 300.0)))),
            (0.2, Box::new(|| session::command(Command::Pin))),
            (0.6, Box::new(|| shot("40-pin"))),
        ],
        "selftest-webcam" => vec![
            (0.0, Box::new(|| {
                crate::settings::update(|s| {
                    s.rec_webcam = true;
                    s.rec_mic = true;
                    s.rec_countdown = false;
                });
                session::start(Mode::Record);
            })),
            (0.8, Box::new(move || drag((x0, y0), (x1, y1)))),
            (0.1, Box::new(|| session::command(Command::StartRecording))),
            (3.5, Box::new(|| shot("50-webcam-recording"))),
            (0.1, Box::new(|| crate::commands::run(Command::StopRecording))),
            (4.0, Box::new(|| {
                crate::settings::update(|s| {
                    s.rec_webcam = false;
                    s.rec_mic = false;
                    s.rec_countdown = true;
                });
                shot("51-webcam-saved");
            })),
        ],
        "selftest-plain" => vec![(0.0, Box::new(|| shot("00-plain")))],
        "selftest-escape" => vec![
            (0.0, Box::new(|| session::start(Mode::Screenshot))),
            (0.8, Box::new(move || drag((x0, y0), (x1, y1)))),
            (0.2, Box::new(|| crate::commands::run(Command::Escape))),
            (0.3, Box::new(|| util::log(format!("after Esc: overlay open = {}", session::is_open())))),
            (0.2, Box::new(|| session::start(Mode::Record))),
            (0.8, Box::new(move || drag((x0, y0), (x1, y1)))),
            (0.1, Box::new(|| session::command(Command::StartRecording))),
            (1.0, Box::new(|| crate::commands::run(Command::Escape))),
            (0.5, Box::new(|| util::log(format!("after Esc in countdown: recording = {}", crate::record::is_active())))),
        ],
        _ => return,
    };
    run_steps(steps);
}

/// Log which formats Photon put on the clipboard.
fn clipboard_report() {
    use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    unsafe {
        let png = RegisterClipboardFormatW(windows::core::w!("PNG"));
        let has_png = IsClipboardFormatAvailable(png).is_ok();
        let has_dib = IsClipboardFormatAvailable(windows::Win32::System::Ole::CF_DIBV5.0 as u32).is_ok();
        let has_bmp = IsClipboardFormatAvailable(windows::Win32::System::Ole::CF_BITMAP.0 as u32).is_ok();
        util::log(format!("clipboard: PNG={has_png} DIBV5={has_dib} BITMAP(synth)={has_bmp}"));
    }
}
