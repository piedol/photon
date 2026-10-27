//! Developer self-test (`photon://selftest` when launched with PHOTON_DEV=1).
//! Drives a scripted session and screenshots each step into $PHOTON_SHOTS.

use crate::commands::Command;
use crate::geometry::pt;
use crate::overlay::toolbar::Mode;
use crate::session::{self, MouseKind};
use crate::util;

fn shot(name: &str) {
    let dir = std::env::var("PHOTON_SHOTS").unwrap_or_else(|_| "/tmp/photon-shots".into());
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("/usr/sbin/screencapture").args(["-x", "-C", &format!("{dir}/{name}.png")]).status();
    util::log(format!("selftest shot {name}"));
}

fn drag(a: (f64, f64), b: (f64, f64), shift: bool) {
    session::mouse_at(0, MouseKind::Down, pt(a.0, a.1), shift);
    for i in 1..=12 {
        let t = i as f64 / 12.0;
        session::mouse_at(0, MouseKind::Drag, pt(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), shift);
    }
    session::mouse_at(0, MouseKind::Up, pt(b.0, b.1), shift);
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
    let rest = util::MainBound((steps, f));
    util::main_after(delay, move || {
        let whole = rest;
        let util::MainBound((steps, f)) = whole;
        f();
        run_steps(steps);
    });
}

pub fn run() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(|| session::start(Mode::Screenshot))),
        (1.0, Box::new(|| session::mouse_at(0, MouseKind::Move, pt(700.0, 600.0), false))),
        (0.3, Box::new(|| shot("01-hover"))),
        (0.2, Box::new(|| drag((260.0, 980.0), (1060.0, 380.0), false))),
        (0.3, Box::new(|| shot("02-selected"))),
        (0.2, Box::new(|| {
            tool(2); // arrow
            drag((320.0, 900.0), (560.0, 760.0), false);
            tool(3); // rect
            drag((600.0, 900.0), (760.0, 800.0), false);
            tool(4); // ellipse
            drag((800.0, 900.0), (980.0, 790.0), true);
            tool(0); // pen
            drag((320.0, 700.0), (500.0, 620.0), false);
            tool(5); // marker
            drag((540.0, 700.0), (760.0, 700.0), false);
            tool(7); // blur
            drag((330.0, 680.0), (700.0, 645.0), false);
            tool(8); // steps
            for x in [340.0, 400.0, 460.0] {
                session::mouse_at(0, MouseKind::Down, pt(x, 520.0), false);
                session::mouse_at(0, MouseKind::Up, pt(x, 520.0), false);
            }
            tool(1); // line
            drag((520.0, 520.0), (700.0, 470.0), true);
            tool(6); // text
            session::mouse_at(0, MouseKind::Down, pt(740.0, 540.0), false);
            session::mouse_at(0, MouseKind::Up, pt(740.0, 540.0), false);
            session::set_text_for_test("Photon text");
        })),
        (0.4, Box::new(|| shot("03-markup"))),
        (0.1, Box::new(|| {
            let _ = session::text_finished(true);
            session::command(Command::ToggleSwatches);
        })),
        (0.3, Box::new(|| shot("04-swatches"))),
        (0.1, Box::new(|| {
            session::command(Command::ToggleSwatches);
            session::command(Command::Copy);
        })),
        (0.3, Box::new(|| shot("05-copied"))),
        (1.5, Box::new(|| session::start(Mode::Record))),
        (1.0, Box::new(|| drag((400.0, 900.0), (1400.0, 300.0), false))),
        (0.3, Box::new(|| shot("06-record-mode"))),
        (0.1, Box::new(session::close)),
    ];
    run_steps(steps);
}

pub fn run_record() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(|| session::start(Mode::Record))),
        (1.0, Box::new(|| drag((300.0, 1000.0), (1300.0, 400.0), false))),
        (0.3, Box::new(|| shot("10-record-selected"))),
        (0.1, Box::new(|| session::command(Command::StartRecording))),
        (1.2, Box::new(|| shot("11-countdown"))),
        (3.5, Box::new(|| shot("12-recording"))),
        (2.0, Box::new(crate::record::toggle_pause)),
        (1.0, Box::new(crate::record::toggle_pause)),
        (2.0, Box::new(|| crate::record::stop(false))),
        (8.0, Box::new(|| shot("13-finished"))),
        (0.2, Box::new(|| crate::record::post::command(Command::Trim))),
        (2.5, Box::new(|| shot("15-trim"))),
        (0.2, Box::new(crate::record::post::close_trim)),
    ];
    run_steps(steps);
}

pub fn run_settings() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(crate::settings_window::show)),
        (0.3, Box::new(crate::settings_window::move_for_test)),
        (0.5, Box::new(|| shot("20-general"))),
        (0.1, Box::new(|| crate::settings_window::select_tab(1))),
        (0.5, Box::new(|| shot("21-screenshots"))),
        (0.1, Box::new(|| crate::settings_window::select_tab(2))),
        (0.5, Box::new(|| shot("22-recordings"))),
        (0.1, Box::new(crate::settings_window::close_for_test)),
    ];
    run_steps(steps);
}

pub fn run_window() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let click = |x: f64, y: f64| {
        session::mouse_at(0, MouseKind::Move, pt(x, y), false);
        session::mouse_at(0, MouseKind::Down, pt(x, y), false);
        session::mouse_at(0, MouseKind::Up, pt(x, y), false);
    };
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(|| session::start(Mode::Record))),
        (1.0, Box::new(move || click(700.0, 600.0))),
        (0.3, Box::new(|| shot("30-window-picked"))),
        (0.1, Box::new(|| session::command(Command::StartRecording))),
        (0.8, Box::new(|| shot("31-countdown-skip"))),
        (0.1, Box::new(|| crate::commands::run(Command::SkipCountdown))),
        (3.0, Box::new(|| shot("32-window-recording"))),
        // The screenshot shortcut must end a running recording.
        (0.1, Box::new(|| crate::commands::run(Command::CaptureScreenshot))),
        (3.0, Box::new(|| shot("33-stopped"))),
    ];
    run_steps(steps);
}

pub fn run_escape() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(|| session::start(Mode::Screenshot))),
        (1.0, Box::new(|| drag((300.0, 900.0), (900.0, 400.0), false))),
        (0.2, Box::new(|| crate::commands::run(Command::Escape))),
        (0.3, Box::new(|| util::log(format!("after Esc: overlay open = {}", session::is_open())))),
        (0.2, Box::new(|| session::start(Mode::Record))),
        (1.0, Box::new(|| drag((300.0, 900.0), (900.0, 400.0), false))),
        (0.1, Box::new(|| session::command(Command::StartRecording))),
        (1.0, Box::new(|| crate::commands::run(Command::Escape))),
        (0.5, Box::new(|| util::log(format!("after Esc in countdown: recording = {}", crate::record::is_active())))),
    ];
    run_steps(steps);
}

/// A crop of the screen, without the cursor, in points from the primary display's top-left
/// corner, saved as $PHOTON_SHOTS/<name>.png (2x on a Retina display).
fn grab(name: &str, x: f64, y: f64, w: f64, h: f64) {
    let dir = std::env::var("PHOTON_SHOTS").unwrap_or_else(|_| "/tmp/photon-shots".into());
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", &format!("-R{x},{y},{w},{h}"), &format!("{dir}/{name}.png")])
        .status();
    util::log(format!("selftest grab {name}"));
}

/// One window, without its shadow, as $PHOTON_SHOTS/<name>.png.
fn grab_window(name: &str, number: isize) {
    let dir = std::env::var("PHOTON_SHOTS").unwrap_or_else(|_| "/tmp/photon-shots".into());
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::process::Command::new("/usr/sbin/screencapture")
        .args(["-x", "-o", "-l", &number.to_string(), &format!("{dir}/{name}.png")])
        .status();
    util::log(format!("selftest grab {name}"));
}

/// `photon://selftest-showcase`: the README's overlay shots, docs/images/overlay.png and
/// docs/images/markup.png (the same markup with the colour swatches open).
/// Start `swift scripts/showcase-backdrop.swift` first: it centres an 820 × 590 pt page on the
/// primary display. This selects the page's (80, 80, 640, 420) rect, marks it up, and saves a
/// 728 × 532 pt crop holding the selection, its size badge, the tool palette and the action bar.
/// The numbers match `page`, `selection` and `crop` in that script.
pub fn run_showcase() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let mtm = objc2::MainThreadMarker::new().unwrap();
    let Some(screen) = objc2_app_kit::NSScreen::screens(mtm).firstObject().map(|s| s.frame()) else {
        return;
    };
    let (w, h) = (screen.size.width, screen.size.height);
    // The page's top-left corner in view points (bottom-left origin).
    let (left, top) = ((w - 820.0) / 2.0, (h - 590.0) / 2.0 + 590.0);
    // Page points (top-left origin) to view points.
    let p = move |x: f64, y: f64| (left + x, top - y);
    let click = move |x: f64, y: f64| {
        let (vx, vy) = p(x, y);
        session::mouse_at(0, MouseKind::Down, pt(vx, vy), false);
        session::mouse_at(0, MouseKind::Up, pt(vx, vy), false);
    };
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(|| session::start(Mode::Screenshot))),
        (1.0, Box::new(move || drag(p(80.0, 500.0), p(720.0, 80.0), false))),
        (0.3, Box::new(move || {
            session::command(Command::Color(0)); // red
            session::command(Command::Stroke(1)); // medium
            tool(3); // a box around Wednesday and Thursday
            drag(p(219.0, 358.0), p(317.0, 188.0), false);
            tool(8); // step 1 on its corner
            click(219.0, 188.0);
            tool(2); // an arrow from the note to the box
            drag(p(566.0, 438.0), p(324.0, 296.0), false);
            tool(6); // the note, its top-left at the click
            click(520.0, 444.0);
            session::set_text_for_test("Best days yet");
        })),
        (0.2, Box::new(move || {
            let _ = session::text_finished(true);
            session::command(Command::Color(2)); // yellow
            session::command(Command::Stroke(2));
            tool(5); // highlight the top referrer
            drag(p(138.0, 423.0), p(236.0, 423.0), false);
            session::command(Command::Color(0));
            session::command(Command::Stroke(1));
            tool(7); // pixelate an address
            drag(p(256.0, 438.0), p(396.0, 462.0), false);
            tool(8); // step 2 beside it
            click(414.0, 450.0);
            tool(2); // leave the arrow selected in the palette
        })),
        (0.8, Box::new(move || grab("overlay", left + 56.0, h - top + 32.0, 728.0, 532.0))),
        // markup.png: the colour swatches open beside the palette, over the right of the page.
        (0.1, Box::new(|| session::command(Command::ToggleSwatches))),
        (0.5, Box::new(move || grab("markup", left + 488.0, h - top + 132.0, 548.0, 424.0))),
        (0.2, Box::new(session::close)),
        (0.2, Box::new(|| util::log("selftest done"))),
    ];
    run_steps(steps);
}

/// `photon://selftest-settings-shots`: each Settings tab as a window capture without its
/// shadow: settings-general.png, settings-screenshots.png, settings-recordings.png.
pub fn run_settings_shots() {
    if std::env::var_os("PHOTON_DEV").is_none() {
        return;
    }
    let shoot = |name: &'static str| {
        move || {
            if let Some(n) = crate::settings_window::window_number_for_test() {
                grab_window(name, n);
            }
        }
    };
    let steps: Vec<(f64, Step)> = vec![
        (0.0, Box::new(crate::settings_window::show)),
        (0.8, Box::new(shoot("settings-general"))),
        (0.1, Box::new(|| crate::settings_window::select_tab(1))),
        (0.6, Box::new(shoot("settings-screenshots"))),
        (0.1, Box::new(|| crate::settings_window::select_tab(2))),
        (0.6, Box::new(shoot("settings-recordings"))),
        (0.1, Box::new(crate::settings_window::close_for_test)),
        (0.2, Box::new(|| util::log("selftest done"))),
    ];
    run_steps(steps);
}
