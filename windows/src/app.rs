//! App lifecycle: single instance, the `photon://` URL scheme, Launch at Login,
//! the tray icon and menu, the message loop, and command routing.

use std::cell::RefCell;
use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, HGDIOBJ, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_BALLOONUSERCLICK, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, ShellExecuteW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW, FindWindowW,
    GetCursorPos, GetMessageW, GetSystemMetrics, HICON, HWND_MESSAGE, ICONINFO, IMAGE_ICON, LR_DEFAULTCOLOR,
    CreateIconIndirect, LoadImageW, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SM_CXSMICON, SW_SHOWNORMAL, SendMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_HOTKEY, WM_LBUTTONUP, WM_NULL,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};
use windows::core::{w, PCWSTR};

use crate::commands::Command;
use crate::d2d::{self, rgba};
use crate::i18n::{tr, trf};
use crate::toolbar::Mode;
use crate::util::{self, W};
use crate::{hotkey, pin, record, session, settings, settings_window, update_ui};

const WM_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const URL_SCHEME: &str = "photon";

thread_local! {
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static ICONS: RefCell<Option<(HICON, HICON)>> = const { RefCell::new(None) };
    static RECORDING_UI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Photon.exe"))
}

// ── Registry helpers ───────────────────────────────────────────────────────

fn reg_set(path: &str, name: Option<&str>, value: &str) -> bool {
    unsafe {
        let mut h = HKEY::default();
        let p = W::new(path);
        if RegCreateKeyExW(HKEY_CURRENT_USER, p.p(), Some(0), None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut h, None).is_err() {
            return false;
        }
        let v = util::wide(value);
        let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
        let n = name.map(W::new);
        let ok = RegSetValueExW(h, n.as_ref().map(|n| n.p()).unwrap_or(PCWSTR::null()), Some(0), REG_SZ, Some(bytes)).is_ok();
        let _ = RegCloseKey(h);
        ok
    }
}

fn reg_get(path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut h = HKEY::default();
        let p = W::new(path);
        if RegOpenKeyExW(HKEY_CURRENT_USER, p.p(), Some(0), KEY_READ, &mut h).is_err() {
            return None;
        }
        let n = W::new(name);
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let ok = RegQueryValueExW(h, n.p(), None, None, Some(buf.as_mut_ptr() as *mut u8), Some(&mut len)).is_ok();
        let _ = RegCloseKey(h);
        ok.then(|| util::from_wide(&buf))
    }
}

fn reg_delete(path: &str, name: &str) -> bool {
    unsafe {
        let mut h = HKEY::default();
        let p = W::new(path);
        if RegOpenKeyExW(HKEY_CURRENT_USER, p.p(), Some(0), KEY_WRITE, &mut h).is_err() {
            return false;
        }
        let n = W::new(name);
        let ok = RegDeleteValueW(h, n.p()).is_ok();
        let _ = RegCloseKey(h);
        ok
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub fn login_item_enabled() -> bool {
    reg_get(RUN_KEY, "Photon").is_some()
}

pub fn set_login_item(on: bool) -> Result<(), String> {
    let ok = if on {
        reg_set(RUN_KEY, Some("Photon"), &format!("\"{}\"", exe_path().display()))
    } else {
        reg_delete(RUN_KEY, "Photon") || !login_item_enabled()
    };
    if ok { Ok(()) } else { Err(tr("Windows refused the change.")) }
}

pub fn toggle_login_item() {
    if let Err(e) = set_login_item(!login_item_enabled()) {
        crate::hud::message(&tr("Couldn't change Launch at Login"), Some(&e));
    }
}

/// Register `photon://` for this user, pointing at this exe.
fn register_url_scheme() {
    let base = format!("Software\\Classes\\{URL_SCHEME}");
    reg_set(&base, None, "URL:Photon");
    reg_set(&base, Some("URL Protocol"), "");
    reg_set(&format!("{base}\\DefaultIcon"), None, &format!("\"{}\",0", exe_path().display()));
    reg_set(&format!("{base}\\shell\\open\\command"), None, &format!("\"{}\" \"%1\"", exe_path().display()));
}

// ── Entry ──────────────────────────────────────────────────────────────────

pub fn main(mut args: Vec<String>) -> i32 {
    // `--updated <pid>`: started by the version this one replaced. Wait for it to quit, so
    // the single-instance check below doesn't find it.
    if let Some(i) = args.iter().position(|a| a == "--updated") {
        if let Some(pid) = args.get(i + 1).and_then(|p| p.parse().ok()) {
            crate::update::wait_for_exit(pid);
        }
        args.drain(i..(i + 2).min(args.len()));
    }

    // `Photon --login-item on|off|status`, like the Mac build.
    if args.first().map(String::as_str) == Some("--login-item") {
        let r = match args.get(1).map(String::as_str) {
            Some("on") => set_login_item(true),
            Some("off") => set_login_item(false),
            _ => Ok(()),
        };
        return match r {
            Ok(()) => {
                println!("Launch at Login: {}", if login_item_enabled() { "on" } else { "off" });
                0
            }
            Err(e) => {
                eprintln!("Couldn't change Launch at Login: {e}");
                1
            }
        };
    }

    // One instance: later launches (e.g. from a photon:// link) forward and exit.
    let instance = unsafe { CreateMutexW(None, false, w!("Local\\PhotonSingleInstance")) };
    unsafe {
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(hwnd) = FindWindowW(w!("PhotonMessage"), None) {
                let text = args.join(" ");
                let data: Vec<u16> = text.encode_utf16().collect();
                let cds = COPYDATASTRUCT { dwData: 0x50484F54, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as _ };
                SendMessageW(hwnd, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&cds as *const _ as isize)));
            }
            return 0;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    register_url_scheme();
    let hwnd = create_message_window();
    util::set_message_window(hwnd);
    TASKBAR_CREATED.with(|c| c.set(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }));
    add_tray_icon();
    apply_hotkeys();
    crate::destination::clean_scratch();
    util::log("launched");
    update_ui::launched();

    // A photon:// URL on the first launch.
    for a in &args {
        open_url(a);
    }

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if settings_window::is_dialog_message(&msg) || update_ui::is_dialog_message(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    remove_tray_icon();
    if crate::update::pending() {
        // The new version starts now. Let go of the shortcuts and the single-instance lock
        // first, so it gets them instead of finding this copy still running.
        hotkey::suspend();
        hotkey::set_escape(false);
        if let Ok(h) = instance {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
        update_ui::quitting();
    }
    0
}

fn create_message_window() -> HWND {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: w!("PhotonMessage"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // A hidden top-level window (not HWND_MESSAGE) so FindWindow and the tray can reach it.
        let _ = HWND_MESSAGE;
        CreateWindowExW(Default::default(), w!("PhotonMessage"), w!("Photon"), Default::default(), 0, 0, 0, 0, None, None, Some(hinst.into()), None)
            .expect("message window")
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        util::WM_RUN_QUEUED => {
            util::drain_queue();
            return LRESULT(0);
        }
        WM_TIMER => {
            if util::fire_timer(wp.0) {
                return LRESULT(0);
            }
        }
        WM_HOTKEY => {
            if let Some(cmd) = hotkey::command_for(wp.0 as i32) {
                crate::commands::run(cmd);
            }
            return LRESULT(0);
        }
        WM_TRAY => {
            let event = (lp.0 & 0xffff) as u32;
            match event {
                WM_LBUTTONUP => {
                    if record::is_active() {
                        record::stop(false);
                    } else {
                        tray_menu(hwnd);
                    }
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(hwnd),
                // Only update notifications use balloons.
                NIN_BALLOONUSERCLICK => util::main_async(update_ui::notification_clicked),
                _ => {}
            }
            return LRESULT(0);
        }
        WM_COPYDATA => {
            let cds = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
            if cds.dwData == 0x50484F54 && !cds.lpData.is_null() {
                let s = unsafe { std::slice::from_raw_parts(cds.lpData as *const u16, (cds.cbData / 2) as usize) };
                let text = String::from_utf16_lossy(s);
                util::main_async(move || {
                    for part in text.split_whitespace() {
                        open_url(part);
                    }
                });
            }
            return LRESULT(1);
        }
        WM_COMMAND => {}
        m if m == TASKBAR_CREATED.with(|c| c.get()) && m != 0 => {
            // Explorer restarted: put the tray icon back.
            add_tray_icon();
            refresh_recording_ui();
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// `photon://capture`, `photon://record`, `photon://stop`, `photon://settings`.
pub fn open_url(url: &str) {
    let host = url.trim().trim_start_matches("photon://").trim_start_matches("photon:").trim_end_matches('/');
    let cmd = match host {
        "capture" | "screenshot" => Command::CaptureScreenshot,
        "record" => Command::RecordScreen,
        "stop" => Command::StopRecording,
        "settings" => Command::OpenSettings,
        h if h.starts_with("selftest") => return crate::selftest::run(h),
        _ => return,
    };
    handle(cmd);
}

/// (Re)register hotkeys from settings.
pub fn apply_hotkeys() -> Vec<String> {
    let s = settings::get();
    let errors = hotkey::install(s.shot_hotkey, s.record_hotkey);
    for e in &errors {
        util::log(e);
    }
    errors
}

// ── Tray ───────────────────────────────────────────────────────────────────

fn app_icon(size: i32) -> HICON {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(Some(hinst.into()), crate::util::int_resource(1), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default()
    }
}

/// A red recording dot drawn at the tray icon size.
fn recording_icon(size: i32) -> HICON {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return HICON::default();
        };
        let old: HGDIOBJ = SelectObject(dc, color.into());
        if let Ok(rt) = d2d::factory().CreateDCRenderTarget(&d2d::premultiplied_props()) {
            let r = windows::Win32::Foundation::RECT { left: 0, top: 0, right: size, bottom: size };
            if rt.BindDC(dc, &r).is_ok() {
                let rt: windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = windows_core::Interface::cast(&rt).unwrap();
                rt.BeginDraw();
                rt.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
                let c = size as f64 / 2.0;
                rt.FillEllipse(&d2d::ellipse(c, c, c - 1.0, c - 1.0), &d2d::brush(&rt, rgba(1.0, 1.0, 1.0, 1.0)));
                rt.FillEllipse(&d2d::ellipse(c, c, c - 2.5, c - 2.5), &d2d::brush(&rt, rgba(0.92, 0.20, 0.17, 1.0)));
                let _ = rt.EndDraw(None, None);
            }
        }
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn icons() -> (HICON, HICON) {
    ICONS.with(|i| {
        *i.borrow_mut().get_or_insert_with(|| {
            let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
            (app_icon(size), recording_icon(size))
        })
    })
}

fn tray_data(tip: &str, recording: bool) -> NOTIFYICONDATAW {
    let (normal, rec) = icons();
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: util::message_window(),
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: if recording { rec } else { normal },
        ..Default::default()
    };
    let t: Vec<u16> = tip.encode_utf16().take(127).collect();
    nid.szTip[..t.len()].copy_from_slice(&t);
    nid
}

fn add_tray_icon() {
    let mut nid = tray_data("Photon", false);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

fn remove_tray_icon() {
    let nid = tray_data("Photon", false);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
    ICONS.with(|i| {
        if let Some((a, b)) = i.borrow_mut().take() {
            unsafe {
                let _ = DestroyIcon(a);
                let _ = DestroyIcon(b);
            }
        }
    });
}

/// A notification from the tray icon (a toast on Windows 10 and 11). Clicking it sends
/// `NIN_BALLOONUSERCLICK`.
pub fn balloon(title: &str, text: &str) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: util::message_window(),
        uID: TRAY_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_USER | NIIF_LARGE_ICON,
        hBalloonIcon: balloon_icon(),
        ..Default::default()
    };
    let t: Vec<u16> = title.encode_utf16().take(nid.szInfoTitle.len() - 1).collect();
    nid.szInfoTitle[..t.len()].copy_from_slice(&t);
    let b: Vec<u16> = text.encode_utf16().take(nid.szInfo.len() - 1).collect();
    nid.szInfo[..b.len()].copy_from_slice(&b);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// The app icon at the large-icon size, loaded once and kept for the notification.
fn balloon_icon() -> HICON {
    thread_local! {
        static ICON: std::cell::Cell<Option<HICON>> = const { std::cell::Cell::new(None) };
    }
    ICON.with(|i| {
        let icon = i.get().unwrap_or_else(|| app_icon(unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXICON) }));
        i.set(Some(icon));
        icon
    })
}

/// Quit at once: the update is in place, and the new version starts as this one exits.
pub fn quit_now() {
    unsafe { PostQuitMessage(0) };
}

/// Nothing would be lost by quitting now: no capture, recording or video in progress, no
/// pinned screenshot, and no file or print dialog open. An update waits for this.
pub fn is_idle() -> bool {
    !session::is_open() && !record::is_active() && !util::busy() && !pin::any() && !record::post::trimming()
}

/// Switch the tray icon between idle and recording.
pub fn set_recording_ui(on: bool) {
    RECORDING_UI.with(|c| c.set(on));
    refresh_recording_ui();
}

pub fn refresh_recording_ui() {
    let on = RECORDING_UI.with(|c| c.get());
    let tip = if on {
        let secs = record::elapsed().as_secs();
        let time = format!("{:02}:{:02}", secs / 60, secs % 60);
        if record::is_paused() {
            trf("Photon: paused {time} (click to stop)", &[("time", &time)])
        } else {
            trf("Photon: recording {time} (click to stop)", &[("time", &time)])
        }
    } else {
        "Photon".to_string()
    };
    let nid = tray_data(&tip, on);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

const M_SHOT: usize = 1;
const M_RECORD: usize = 2;
const M_FOLDER: usize = 3;
const M_SETTINGS: usize = 4;
const M_LOGIN: usize = 5;
const M_QUIT: usize = 6;
const M_UPDATE: usize = 7;
const M_UPDATE_STATUS: usize = 8;
const M_STOP: usize = 10;
const M_PAUSE: usize = 11;
const M_DISCARD: usize = 12;

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let s = settings::get();
        let add = |id: usize, text: &str, flags| {
            let t = W::new(text);
            let _ = AppendMenuW(menu, flags, id, t.p());
        };
        if record::is_active() {
            add(M_STOP, &tr("Stop and Save"), MF_STRING);
            add(M_PAUSE, &if record::is_paused() { tr("Resume") } else { tr("Pause") }, MF_STRING);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            add(M_DISCARD, &tr("Discard Recording"), MF_STRING);
        } else {
            // The shortcut after the tab is Windows' accelerator column, not part of the text.
            add(M_SHOT, &format!("{}\t{}", tr("Capture Screenshot"), s.shot_hotkey.label()), MF_STRING);
            add(M_RECORD, &format!("{}\t{}", tr("Record Screen"), s.record_hotkey.label()), MF_STRING);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            add(M_FOLDER, &tr("Open Recordings Folder"), MF_STRING);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            add(M_SETTINGS, &tr("Settings…"), MF_STRING);
            add(M_LOGIN, &tr("Launch at Login"), if login_item_enabled() { MF_STRING | MF_CHECKED } else { MF_STRING });
            let (title, status, enabled) = update_ui::menu_item();
            add(M_UPDATE, &title, if enabled { MF_STRING } else { MF_STRING | MF_GRAYED });
            if let Some(status) = status {
                add(M_UPDATE_STATUS, &status, MF_STRING | MF_GRAYED);
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            add(M_QUIT, &tr("Quit Photon"), if session::is_open() { MF_STRING | MF_GRAYED } else { MF_STRING });
        }
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), hwnd, None);
        let _ = SendMessageW(hwnd, WM_NULL, None, None);
        let _ = DestroyMenu(menu);
        let c = match cmd.0 as usize {
            M_SHOT => Command::CaptureScreenshot,
            M_RECORD => Command::RecordScreen,
            M_FOLDER => Command::OpenRecordings,
            M_SETTINGS => Command::OpenSettings,
            M_LOGIN => Command::ToggleLogin,
            M_QUIT => Command::Quit,
            M_UPDATE => Command::Update(0),
            M_STOP => Command::StopRecording,
            M_PAUSE => Command::PauseResume,
            M_DISCARD => Command::Setting(999),
            _ => return,
        };
        // Let the menu finish closing before opening overlays.
        util::main_async(move || crate::commands::run(c));
    }
}

/// Open a file or folder with the default app.
pub fn shell_open(path: &str) {
    let p = W::new(path);
    unsafe {
        ShellExecuteW(None, w!("open"), p.p(), None, None, SW_SHOWNORMAL);
    }
}

// ── Command routing ────────────────────────────────────────────────────────

pub fn handle(cmd: Command) {
    match cmd {
        // While recording, either shortcut ends the recording.
        Command::CaptureScreenshot | Command::RecordHotkey if record::is_active() => record::stop(false),
        Command::CaptureScreenshot => session::start(Mode::Screenshot),
        Command::RecordScreen | Command::RecordHotkey => session::start(Mode::Record),
        Command::SkipCountdown => record::skip_countdown(),
        Command::Escape => {
            if session::is_open() {
                session::escape();
            } else if record::is_counting_down() {
                record::stop(true);
            }
        }
        Command::Quit => {
            if record::is_active() {
                record::stop(false);
                util::main_after(2.0, || unsafe { PostQuitMessage(0) });
            } else {
                unsafe { PostQuitMessage(0) };
            }
        }
        Command::StopRecording => record::stop(false),
        Command::PauseResume => record::toggle_pause(),
        Command::Setting(999) => record::stop(true),
        Command::Setting(id) => settings_window::changed(id),
        Command::ToggleLogin => toggle_login_item(),
        Command::Update(n) => update_ui::command(n),
        Command::OpenSettings => settings_window::show(),
        Command::OpenRecordings => {
            let dir = settings::get().rec_dir;
            let _ = std::fs::create_dir_all(&dir);
            shell_open(&dir.to_string_lossy());
        }
        Command::Reveal | Command::CopyFile | Command::OpenFile | Command::Trim | Command::ExportGif | Command::DismissHud => {
            record::post::command(cmd)
        }
        c if session::is_open() => session::command(c),
        _ => util::log(format!("unhandled {cmd:?}")),
    }
}
