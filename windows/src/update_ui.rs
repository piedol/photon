//! Photon's side of the updater (the Windows core is in update/): the tray menu item, the
//! daily check and its setting, the notification, and the update window. The core's
//! messages are English, for the log; what Photon shows goes through `tr`.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::sync::Once;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET, DT_CALCRECT, DT_NOPREFIX,
    DT_WORDBREAK, DeleteObject, DrawTextW, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, GetDC, GetSysColorBrush,
    GetTextExtentPoint32W, HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_NOPREFIX;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, HMENU, HWND_NOTOPMOST, HWND_TOPMOST, IDCANCEL, IDOK, IsDialogMessageW, IsWindow, LoadIconW, MSG,
    RegisterClassExW, SW_SHOW, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_SETFONT, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, w};

use crate::i18n::{tr, trf};
use crate::update::notes::{Line, Span};
use crate::update::{self, Check, Release, Step};
use crate::util::{self, W};
use crate::{app, hud, pin, settings};

pub static CONFIG: update::Config = update::Config {
    repo: "piedol/photon",
    asset: "Photon-windows-x64.zip",
    name: "Photon",
    env_prefix: "PHOTON",
    version: env!("CARGO_PKG_VERSION"),
    log: |m| util::log(m),
};

/// What the menu item offers.
enum State {
    Idle,
    /// A check chosen from the menu is running.
    Checking,
    Available(Release),
    Installing(Release, Step),
}

// `Command::Update(n)`: the menu item, then the update window's buttons.
const MENU: u8 = 0;
const INSTALL: u8 = 1;
const LATER: u8 = 2;
const OK: u8 = 3;

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Idle) };
    /// The version last announced in a notification, so each is announced once per run.
    static NOTIFIED: RefCell<Option<update::Version>> = const { RefCell::new(None) };
    /// PHOTON_UPDATE_TEST=install: unattended, so results go to the log only.
    static TESTING: Cell<bool> = const { Cell::new(false) };
}

fn current() -> update::Version {
    update::current_version(&CONFIG)
}

fn set_state(s: State) {
    STATE.with(|st| *st.borrow_mut() = s);
}

fn available() -> Option<Release> {
    STATE.with(|st| match &*st.borrow() {
        State::Available(r) | State::Installing(r, _) => Some(r.clone()),
        _ => None,
    })
}

/// Called from `app::main` as Photon exits: an installed update starts now.
pub fn quitting() {
    update::start_pending(&CONFIG);
}

/// Called from `app::main` once the tray icon is up.
pub fn launched() {
    if let Some(v) = update::launched(&CONFIG) {
        hud::toast(&trf("Updated to Photon {version}", &[("version", &v.to_string())]), None);
    }
    // For testing the whole install: check now and install anything newer without asking.
    // Nothing is saved to settings in this mode.
    if std::env::var("PHOTON_UPDATE_TEST").as_deref() == Ok("install") {
        TESTING.with(|t| t.set(true));
        util::log("update test: checking now, installing without asking");
        update::check(&CONFIG, |result| match result {
            Ok(Check::Available(r)) => {
                set_state(State::Available(r.clone()));
                install(r);
            }
            Ok(Check::UpToDate) => util::log("update test: nothing newer"),
            Err(e) => util::log(format!("update test: {e}")),
        });
        return;
    }
    start_schedule();
}

fn start_schedule() {
    update::schedule::start(|| settings::get().check_updates, last_check, auto_check);
}

/// The Settings checkbox changed.
pub fn setting_changed() {
    start_schedule();
}

fn last_check() -> Option<SystemTime> {
    let secs = settings::get().last_update_check;
    (secs > 0).then(|| UNIX_EPOCH + Duration::from_secs(secs))
}

fn auto_check() {
    if STATE.with(|s| matches!(&*s.borrow(), State::Checking | State::Installing(..))) {
        return;
    }
    update::check(&CONFIG, |result| checked(result, false));
}

/// Check for Updates… in the menu.
fn check_now() {
    set_state(State::Checking);
    update::check(&CONFIG, |result| checked(result, true));
}

fn checked(result: Result<Check, update::Error>, manual: bool) {
    if result.is_ok() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        settings::update(|s| s.last_update_check = now);
    }
    if update::installing() {
        return;
    }
    match result {
        Ok(Check::Available(r)) => {
            set_state(State::Available(r.clone()));
            if manual {
                show_update(&r);
            } else {
                notify(&r);
            }
        }
        Ok(Check::UpToDate) => {
            set_state(State::Idle);
            if manual {
                message(
                    &tr("Photon is up to date"),
                    &trf("Photon {version} is the newest version.", &[("version", &current().to_string())]),
                );
            }
        }
        Err(e) => {
            if STATE.with(|s| matches!(&*s.borrow(), State::Checking)) {
                set_state(State::Idle);
            }
            if manual {
                message(&tr("Couldn't check for updates"), &explain(&e));
            }
        }
    }
}

// ---------------------------------------------------------------- what went wrong

/// An error from the core in Photon's language. The core's English goes to the log.
fn explain(e: &update::Error) -> String {
    use update::Error;
    match e {
        // Windows' own description, already in Windows' language.
        Error::Network(why) => trf("Couldn't reach GitHub. {reason}", &[("reason", why)]),
        Error::Http(403 | 429) => tr("GitHub is limiting requests from this network. Try again in an hour."),
        Error::Http(404) => tr("GitHub has no published release to compare with."),
        Error::Http(code) => trf("GitHub answered with HTTP status {code}. Try again later.", &[("code", &code.to_string())]),
        Error::Release(_) => tr("GitHub's answer isn't a release Photon can install."),
        Error::Verify(why) => verify_text(why),
        Error::NotWritable(_) => {
            let exe = app::exe_path();
            let folder = exe.parent().unwrap_or(Path::new("")).to_path_buf();
            trf("Photon can't replace itself because your account can't make changes in {folder}.", &[("folder", &util::tilde(&folder))])
        }
        Error::Replace(_) => tr("The new version couldn't be put in place."),
    }
}

/// The core's Verify messages, told apart by their wording; any other gets the general sentence.
fn verify_text(why: &str) -> String {
    if why.contains("doesn't match the checksum") {
        tr("The download doesn't match the checksum GitHub lists for it.")
    } else if why.contains("doesn't list a checksum") || why.contains("isn't a SHA-256") {
        tr("The release doesn't list a checksum for the download, so it can't be checked.")
    } else if why.contains("isn't newer") {
        tr("The downloaded app isn't newer than the one you have.")
    } else {
        tr("The download couldn't be checked, so it wasn't installed.")
    }
}

// ---------------------------------------------------------------- tray menu

/// The tray menu item: (title, a grayed line under it, enabled).
pub fn menu_item() -> (String, Option<String>, bool) {
    STATE.with(|st| match &*st.borrow() {
        State::Idle => (tr("Check for Updates…"), None, true),
        State::Checking => (tr("Checking for Updates…"), None, false),
        State::Available(r) => (
            trf("Update to Photon {version}…", &[("version", &r.version.to_string())]),
            Some(trf("You have {version}", &[("version", &current().to_string())])),
            true,
        ),
        State::Installing(r, step) => (
            trf("Installing Photon {version}…", &[("version", &r.version.to_string())]),
            Some(step_text(*step, r.size)),
            false,
        ),
    })
}

fn step_text(step: Step, size: u64) -> String {
    match step {
        Step::Downloading { received, total } => {
            let total = if total > 0 { total } else { size };
            match (received * 100).checked_div(total) {
                Some(percent) => trf("Downloading, {percent}%", &[("percent", &percent.min(100).to_string())]),
                None => tr("Downloading"),
            }
        }
        Step::Verifying => tr("Checking the download"),
        Step::Waiting if pin::any() => tr("Installs when you close pinned screenshots"),
        Step::Waiting => tr("Installs when you finish capturing"),
        Step::Replacing => tr("Replacing the app"),
    }
}

/// `Command::Update(n)`: the menu item or one of the update window's buttons.
pub fn command(n: u8) {
    match n {
        MENU => menu_chosen(),
        _ => button(n),
    }
}

fn menu_chosen() {
    match available() {
        Some(r) if !update::installing() => show_update(&r),
        Some(_) => {}
        None if STATE.with(|s| matches!(&*s.borrow(), State::Idle)) => check_now(),
        None => {}
    }
}

// ---------------------------------------------------------------- installing

fn install(r: Release) {
    set_state(State::Installing(r.clone(), Step::Downloading { received: 0, total: 0 }));
    // A toast would land on top of a capture, so only when Photon is idle.
    if app::is_idle() {
        hud::toast(&trf("Downloading Photon {version}…", &[("version", &r.version.to_string())]), None);
    }
    update::install(&CONFIG, r, update::Hooks { is_idle: app::is_idle, progress, done: installed });
}

fn progress(step: Step) {
    STATE.with(|st| {
        if let State::Installing(_, s) = &mut *st.borrow_mut() {
            *s = step;
        }
    });
}

fn installed(result: Result<(), update::Error>) {
    let Some(r) = available() else { return };
    let version = r.version.to_string();
    match result {
        Ok(()) => app::quit_now(),
        Err(_) if TESTING.with(Cell::get) => set_state(State::Available(r)),
        Err(e @ update::Error::NotWritable(_)) => {
            set_state(State::Available(r.clone()));
            app::shell_open(&r.page);
            message(
                &trf("Download Photon {version} from GitHub", &[("version", &version)]),
                &trf("{reason} The release page is open in your browser.", &[("reason", &explain(&e))]),
            );
        }
        Err(e) => {
            set_state(State::Available(r.clone()));
            message(
                &trf("Photon {version} wasn't installed", &[("version", &version)]),
                &trf("{reason} Photon {version} is still installed.", &[("reason", &explain(&e)), ("version", &current().to_string())]),
            );
        }
    }
}

// ---------------------------------------------------------------- notification

/// Announce an update found by the daily check, once per version per run.
fn notify(r: &Release) {
    let seen = NOTIFIED.with(|n| n.borrow_mut().replace(r.version.clone()));
    if seen.as_ref() == Some(&r.version) {
        return;
    }
    app::balloon(
        &trf("Photon {version} is available", &[("version", &r.version.to_string())]),
        &trf("You have {version}. Click to see what's new and install it.", &[("version", &current().to_string())]),
    );
}

/// The notification was clicked.
pub fn notification_clicked() {
    if let Some(r) = available()
        && !update::installing()
    {
        show_update(&r);
    }
}

// ---------------------------------------------------------------- the update window

/// The version, the installed version and the release notes, with Install and Restart and Later.
fn show_update(r: &Release) {
    present(
        &trf("Photon {version} is available", &[("version", &r.version.to_string())]),
        &trf(
            "You have Photon {version}. Installing quits Photon and opens the new version. Your settings carry over.",
            &[("version", &current().to_string())],
        ),
        Some(&plain_notes(&r.notes)),
        &[(tr("Install and Restart"), INSTALL), (tr("Later"), LATER)],
    );
}

/// A message with an OK button.
fn message(title: &str, text: &str) {
    present(title, text, None, &[(tr("OK"), OK)]);
}

/// A button in the update window or a message (Enter is the first, Esc and Close the last).
fn button(n: u8) {
    close_window();
    if n == INSTALL
        && let Some(r) = available()
        && !update::installing()
    {
        install(r);
    }
}

/// Release notes as plain lines for a text box: headings and paragraphs as they are,
/// bullets with a "•", a blank line between blocks.
fn plain_notes(markdown: &str) -> String {
    let lines = update::notes::parse(markdown);
    if lines.is_empty() {
        return tr("No release notes.");
    }
    let join = |spans: &[Span]| spans.iter().map(|s| s.text.as_str()).collect::<String>();
    lines
        .iter()
        .map(|l| match l {
            Line::Heading(s) | Line::Text(s) => join(s),
            Line::Bullet(s) => format!("•  {}", join(s)),
            Line::Gap => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

struct Window {
    hwnd: HWND,
    fonts: [HFONT; 2],
    /// Enter presses the first, Esc and the close box the last.
    buttons: Vec<u8>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

static REGISTER: Once = Once::new();
const BUTTON_ID: u16 = 100;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xffff) as i32;
            let buttons = WINDOW.with(|w| w.borrow().as_ref().map(|w| w.buttons.clone())).unwrap_or_default();
            let n = match id {
                x if x == IDOK.0 => buttons.first().copied(),
                x if x == IDCANCEL.0 => buttons.last().copied(),
                x if x >= BUTTON_ID as i32 => buttons.get((x - BUTTON_ID as i32) as usize).copied(),
                _ => None,
            };
            if let Some(n) = n {
                util::main_async(move || button(n));
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            let last = WINDOW.with(|w| w.borrow().as_ref().and_then(|w| w.buttons.last().copied()));
            util::main_async(move || button(last.unwrap_or(OK)));
            return LRESULT(0);
        }
        WM_CTLCOLORSTATIC => unsafe {
            // Labels and the read-only notes box on the window's own background.
            SetBkMode(HDC(wp.0 as *mut _), TRANSPARENT);
            return LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize);
        },
        WM_DESTROY => return LRESULT(0),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn font(height_pt: f64, weight: i32, s: f64) -> HFONT {
    let h = -((height_pt * 96.0 * s / 72.0).round() as i32);
    // SAFETY: plain font creation; deleted when the window closes.
    unsafe {
        CreateFontW(h, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"))
    }
}

/// The height `text` wraps to at `width` in `font`.
fn text_height(hwnd: HWND, font: HFONT, text: &str, width: i32) -> i32 {
    // SAFETY: a window DC, released after measuring; the font is selected back out.
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, font.into());
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        let mut r = RECT { left: 0, top: 0, right: width, bottom: 0 };
        DrawTextW(dc, &mut buf, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
        r.bottom - r.top
    }
}

fn text_width(hwnd: HWND, font: HFONT, text: &str) -> i32 {
    // SAFETY: as in `text_height`.
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, font.into());
        let buf: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &buf, &mut size);
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
        size.cx
    }
}

/// Open a window with a title, text, optional notes and buttons, replacing any open one.
/// It isn't modal: the tray menu and the shortcuts keep working while it's open.
fn present(title: &str, text: &str, notes: Option<&str>, buttons: &[(String, u8)]) {
    close_window();
    let hinst = unsafe { GetModuleHandleW(None).unwrap_or_default() };
    REGISTER.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("PhotonUpdate"),
            hbrBackground: HBRUSH(GetSysColorBrush(COLOR_WINDOW).0),
            hIcon: LoadIconW(Some(hinst.into()), util::int_resource(1)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    let (_, work, _) = hud::monitor_at(hud::cursor());
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    // Made on the pointer's monitor first, so its DPI is known before laying out.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("PhotonUpdate"),
            w!("Photon"),
            style,
            work.mid().x as i32,
            work.mid().y as i32,
            100,
            100,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap_or_default()
    };
    if hwnd.0.is_null() {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let s = dpi as f64 / 96.0;
    let px = |v: f64| (v * s).round() as i32;
    let fonts = [font(9.0, FW_NORMAL.0 as i32, s), font(12.0, FW_SEMIBOLD.0 as i32, s)];
    let (margin, width) = (px(20.0), px(500.0));
    let inner = width - 2 * margin;
    let child = |class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: u16, font: HFONT| {
        let t = W::new(text);
        // SAFETY: a child of our window; the text outlives the call.
        unsafe {
            let h = CreateWindowExW(ex, class, t.p(), WS_CHILD | WS_VISIBLE | style, r.0, r.1, r.2, r.3, Some(hwnd), Some(HMENU(id as usize as *mut _)), Some(hinst.into()), None)
                .unwrap_or_default();
            SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            h
        }
    };
    let mut y = margin;
    let h = text_height(hwnd, fonts[1], title, inner);
    child(w!("STATIC"), title, WINDOW_STYLE(SS_NOPREFIX.0), WINDOW_EX_STYLE::default(), (margin, y, inner, h), 0, fonts[1]);
    y += h + px(8.0);
    let h = text_height(hwnd, fonts[0], text, inner);
    child(w!("STATIC"), text, WINDOW_STYLE(SS_NOPREFIX.0), WINDOW_EX_STYLE::default(), (margin, y, inner, h), 0, fonts[0]);
    y += h + px(14.0);
    if let Some(notes) = notes {
        let h = px(220.0);
        let style = WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32) | WS_VSCROLL | WS_TABSTOP;
        child(w!("EDIT"), notes, style, WS_EX_CLIENTEDGE, (margin, y, inner, h), 0, fonts[0]);
        y += h + px(16.0);
    }
    // Buttons on the right, the first (the default) on the left of the row.
    let bh = px(28.0);
    let widths: Vec<i32> = buttons.iter().map(|(t, _)| (text_width(hwnd, fonts[0], t) + px(28.0)).max(px(88.0))).collect();
    let gap = px(8.0);
    let mut x = margin + inner - widths.iter().sum::<i32>() - gap * (buttons.len() as i32 - 1);
    let mut first = HWND::default();
    for (i, ((label, _), bw)) in buttons.iter().zip(&widths).enumerate() {
        let kind = if i == 0 { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON };
        let b = child(w!("BUTTON"), label, WINDOW_STYLE(kind as u32) | WS_TABSTOP, WINDOW_EX_STYLE::default(), (x, y, *bw, bh), BUTTON_ID + i as u16, fonts[0]);
        if i == 0 {
            first = b;
        }
        x += bw + gap;
    }
    y += bh + margin;
    let mut frame = RECT { left: 0, top: 0, right: width, bottom: y };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE::default(), dpi);
        let (fw, fh) = (frame.right - frame.left, frame.bottom - frame.top);
        let _ = SetWindowPos(hwnd, None, (work.mid().x - fw as f64 / 2.0) as i32, (work.mid().y - fh as f64 / 2.0) as i32, fw, fh, SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        // The answer to a check arrives after the tray menu has closed, and Windows may refuse
        // SetForegroundWindow then, leaving the window behind others. Raising it through the
        // topmost band and back puts it on top without keeping it there.
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(first));
    }
    WINDOW.with(|cell| *cell.borrow_mut() = Some(Window { hwnd, fonts, buttons: buttons.iter().map(|b| b.1).collect() }));
}

fn close_window() {
    if let Some(win) = WINDOW.with(|w| w.borrow_mut().take()) {
        unsafe {
            let _ = DestroyWindow(win.hwnd);
            for f in win.fonts {
                let _ = DeleteObject(f.into());
            }
        }
    }
}

/// Tab, Enter and Esc in the update window.
pub fn is_dialog_message(msg: &MSG) -> bool {
    let Some(hwnd) = WINDOW.with(|w| w.borrow().as_ref().map(|w| w.hwnd)) else { return false };
    unsafe { IsWindow(Some(hwnd)).as_bool() && IsDialogMessageW(hwnd, msg).as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_read_as_plain_lines() {
        let md = "Photon 0.2.1 checks for updates.\n\n## Changes\n\n- **Check for Updates…** in the menu.\n- `Photon.exe` replaces\n  itself.";
        assert_eq!(
            plain_notes(md),
            "Photon 0.2.1 checks for updates.\r\n\r\nChanges\r\n\r\n•  Check for Updates… in the menu.\r\n•  Photon.exe replaces itself."
        );
    }

    // Without PHOTON_LANG the catalogs follow Windows' language, so check structure, not words.
    #[test]
    fn progress_counts_up() {
        let half = step_text(Step::Downloading { received: 5, total: 10 }, 0);
        assert!(half.contains("50"), "{half}");
        let quarter = step_text(Step::Downloading { received: 5, total: 0 }, 20);
        assert!(quarter.contains("25"), "{quarter}");
        assert_eq!(step_text(Step::Downloading { received: 0, total: 0 }, 0), tr("Downloading"));
    }

    #[test]
    fn verify_failures_are_told_apart() {
        let says = |why: &str| explain(&update::Error::Verify(why.into()));
        assert_eq!(says("The download doesn't match the checksum GitHub lists for it."), tr("The download doesn't match the checksum GitHub lists for it."));
        assert_eq!(
            says("The release doesn't list a checksum for Photon-windows-x64.zip, so the download can't be checked."),
            tr("The release doesn't list a checksum for the download, so it can't be checked.")
        );
        assert_eq!(says("The downloaded app is version 0.1.0, which isn't newer than 0.2.1."), tr("The downloaded app isn't newer than the one you have."));
        assert_eq!(says("The download doesn't contain Photon.exe."), tr("The download couldn't be checked, so it wasn't installed."));
    }
}
