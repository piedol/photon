//! The Settings window: General, Screenshots and Recordings tabs built from
//! standard Windows controls. Every change saves immediately.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontW, DEFAULT_CHARSET, DeleteObject, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HFONT,
    OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, GetSysColorBrush, COLOR_WINDOW, HBRUSH, SetBkMode, TRANSPARENT, SetTextColor, HDC,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_STANDARD_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, TCIF_TEXT, TCITEMW,
    TCM_GETCURSEL, TCM_INSERTITEMW, TCN_SELCHANGE, WC_TABCONTROLW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, SetFocus};
use windows::Win32::UI::Shell::{FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT,
    CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST, CreateWindowExW, DefWindowProcW, DestroyWindow, EN_CHANGE, ES_AUTOHSCROLL,
    GetWindowTextLengthW, GetWindowTextW, HMENU, IsDialogMessageW, IsWindow, MSG, RegisterClassExW, SW_HIDE, SW_SHOW,
    SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC,
    WM_DESTROY, WM_KEYDOWN, WM_NOTIFY, WM_SETFONT, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL, WINDOW_STYLE,
};
use windows::Win32::System::SystemServices::SS_RIGHT;
use windows::Win32::UI::Controls::BST_CHECKED;
use windows::core::{w, PCWSTR};

use crate::cloud;
use crate::destination::{self, NameParts};
use crate::i18n::{tr, trf};
use crate::keys::{self, Shortcut};
use crate::naming::{Case, DateFormat, Kind, NamingRule, Separator, TimeFormat, TOKENS};
use crate::settings::{self, Codec, Settings};
use crate::util::{self, W};
use crate::{app, hotkey, hud};

// Control ids (also used as `Command::Setting(id)`).
const SHOT_KEY: u16 = 1;
const REC_KEY: u16 = 2;
const LOGIN: u16 = 3;
const SHOT_AUTOSAVE: u16 = 10;
const SHOT_DEST: u16 = 11;
const REC_DEST: u16 = 40;
const REC_ALSO: u16 = 41;
const REC_ALSO_DEST: u16 = 42;
const CODEC: u16 = 70;
const FPS: u16 = 71;
const SYS_AUDIO: u16 = 72;
const MIC: u16 = 73;
const MIC_DEVICE: u16 = 74;
const WEBCAM: u16 = 75;
const CAM_DEVICE: u16 = 76;
const CURSOR: u16 = 77;
const CLICKS: u16 = 78;
const COUNTDOWN: u16 = 79;
const N_TEMPLATE: u16 = 0;
const N_DATE: u16 = 1;
const N_TIME: u16 = 2;
const N_CASE: u16 = 3;
const N_SEP: u16 = 4;
const N_TOKEN: u16 = 10;
const SHOT_NAMING: u16 = 100;
const REC_NAMING: u16 = 200;
const TABS: u16 = 900;

const CTRL_BASE: u16 = 1000;

struct DestOption {
    label: String,
    path: Option<PathBuf>,
}

struct Ctl {
    id: u16,
    hwnd: HWND,
    tab: usize,
}

struct State {
    hwnd: HWND,
    tabs: HWND,
    ctls: Vec<Ctl>,
    statics: Vec<(HWND, usize)>,
    font: HFONT,
    bold: HFONT,
    dests: Vec<(u16, Vec<DestOption>)>,
    previews: Vec<(u16, HWND)>,
    warnings: Vec<(u16, HWND)>,
    note: HWND,
    mics: Vec<(String, String)>,
    cams: Vec<(String, String)>,
    recording_key: Option<u16>,
    tab: usize,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

static REGISTER: Once = Once::new();

fn ctl(id: u16) -> Option<HWND> {
    STATE.with(|s| s.borrow().as_ref()?.ctls.iter().find(|c| c.id == id).map(|c| c.hwnd))
}

fn is_on(id: u16) -> bool {
    ctl(id).is_some_and(|h| unsafe { SendMessageW(h, BM_GETCHECK, None, None) }.0 == BST_CHECKED.0 as isize)
}

fn set_check(h: HWND, on: bool) {
    unsafe {
        SendMessageW(h, BM_SETCHECK, Some(WPARAM(if on { 1 } else { 0 })), None);
    }
}

fn selected(id: u16) -> usize {
    ctl(id).map(|h| unsafe { SendMessageW(h, CB_GETCURSEL, None, None) }.0.max(0) as usize).unwrap_or(0)
}

fn text_of(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h) as usize;
        let mut buf = vec![0u16; n + 1];
        GetWindowTextW(h, &mut buf);
        util::from_wide(&buf)
    }
}

fn set_text(h: HWND, t: &str) {
    let w = W::new(t);
    unsafe {
        let _ = SetWindowTextW(h, w.p());
    }
}

/// Builds controls top-down in one tab.
struct Form<'a> {
    st: &'a mut State,
    tab: usize,
    y: i32,
    s: f64,
    left: i32,
    width: i32,
}

impl Form<'_> {
    fn px(&self, v: f64) -> i32 {
        (v * self.s).round() as i32
    }

    fn create(&mut self, class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: Option<u16>) -> HWND {
        let t = W::new(text);
        let menu = id.map(|i| HMENU((CTRL_BASE + i) as usize as *mut _));
        let h = unsafe {
            CreateWindowExW(ex, class, t.p(), WS_CHILD | style, r.0, r.1, r.2, r.3, Some(self.st.hwnd), menu, Some(GetModuleHandleW(None).unwrap_or_default().into()), None)
                .unwrap_or_default()
        };
        unsafe {
            SendMessageW(h, WM_SETFONT, Some(WPARAM(self.st.font.0 as usize)), Some(LPARAM(1)));
        }
        match id {
            Some(id) => self.st.ctls.push(Ctl { id, hwnd: h, tab: self.tab }),
            None => self.st.statics.push((h, self.tab)),
        }
        h
    }

    fn label_col(&self) -> i32 {
        self.px(150.0)
    }

    fn ctrl_x(&self) -> i32 {
        self.left + self.label_col() + self.px(10.0)
    }

    fn ctrl_w(&self) -> i32 {
        self.width - self.label_col() - self.px(10.0)
    }

    fn section(&mut self, title: &str) {
        if self.y > self.px(60.0) {
            self.y += self.px(10.0);
        }
        let h = self.create(w!("STATIC"), title, WS_VISIBLE, Default::default(), (self.left, self.y, self.width, self.px(20.0)), None);
        unsafe {
            SendMessageW(h, WM_SETFONT, Some(WPARAM(self.st.bold.0 as usize)), Some(LPARAM(1)));
        }
        self.y += self.px(28.0);
    }

    fn label(&mut self, text: &str, h: i32) {
        let y = self.y + (h - self.px(18.0)) / 2;
        self.create(w!("STATIC"), text, WS_VISIBLE | WINDOW_STYLE(SS_RIGHT.0), Default::default(), (self.left, y + self.px(2.0), self.label_col(), self.px(18.0)), None);
    }

    fn checkbox(&mut self, id: u16, text: &str, on: bool) -> HWND {
        let h = self.create(w!("BUTTON"), text, WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), Default::default(), (self.ctrl_x(), self.y, self.ctrl_w(), self.px(22.0)), Some(id));
        set_check(h, on);
        self.y += self.px(28.0);
        h
    }

    fn combo_at(&mut self, id: u16, items: &[String], sel: usize, x: i32, w: i32) -> HWND {
        let h = self.create(w!("COMBOBOX"), "", WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32), Default::default(), (x, self.y, w, self.px(300.0)), Some(id));
        fill_combo(h, items, sel);
        h
    }

    fn combo_row(&mut self, label: &str, id: u16, items: &[String], sel: usize) -> HWND {
        self.label(label, self.px(26.0));
        let (x, w) = (self.ctrl_x(), self.ctrl_w());
        let h = self.combo_at(id, items, sel, x, w);
        self.y += self.px(34.0);
        h
    }

    fn two_combos(&mut self, label: &str, a: (u16, &[String], usize), b: (u16, &[String], usize)) {
        self.label(label, self.px(26.0));
        let half = (self.ctrl_w() - self.px(8.0)) / 2;
        let x = self.ctrl_x();
        self.combo_at(a.0, a.1, a.2, x, half);
        self.combo_at(b.0, b.1, b.2, x + half + self.px(8.0), half);
        self.y += self.px(34.0);
    }

    fn note(&mut self, text: &str) -> HWND {
        let h = self.create(w!("STATIC"), text, WS_VISIBLE, Default::default(), (self.ctrl_x(), self.y - self.px(4.0), self.ctrl_w(), self.px(32.0)), None);
        self.y += self.px(30.0);
        h
    }
}

fn fill_combo(h: HWND, items: &[String], sel: usize) {
    unsafe {
        SendMessageW(h, CB_RESETCONTENT, None, None);
        for i in items {
            let w = W::new(i);
            SendMessageW(h, CB_ADDSTRING, None, Some(LPARAM(w.p().0 as isize)));
        }
        SendMessageW(h, CB_SETCURSEL, Some(WPARAM(sel.min(items.len().saturating_sub(1)))), None);
    }
}

fn dest_options(current: &std::path::Path, default: PathBuf) -> (Vec<DestOption>, usize) {
    let folders = cloud::discover();
    let mut opts = vec![DestOption { label: trf("{folder}  (default)", &[("folder", &util::tilde(&default))]), path: Some(default.clone()) }];
    for f in &folders {
        opts.push(DestOption { label: format!("{} › Photon", f.label), path: Some(f.default_target.clone()) });
    }
    let mut sel = opts.iter().position(|o| o.path.as_deref() == Some(current));
    if sel.is_none() {
        opts.insert(1, DestOption { label: cloud::display_path(current, &folders), path: Some(current.to_path_buf()) });
        sel = Some(1);
    }
    opts.push(DestOption { label: tr("Other folder…"), path: None });
    (opts, sel.unwrap_or(0))
}

fn labels<T: Copy, S: Into<String>>(all: &[T], f: impl Fn(T) -> S) -> Vec<String> {
    all.iter().map(|x| f(*x).into()).collect()
}

fn pos<T: PartialEq>(all: &[T], v: T) -> usize {
    all.iter().position(|x| *x == v).unwrap_or(0)
}

fn naming_block(f: &mut Form, base: u16, rule: &NamingRule) {
    f.label(&tr("File name:"), f.px(24.0));
    let (x, w) = (f.ctrl_x(), f.ctrl_w());
    let y = f.y;
    let edit = f.create(w!("EDIT"), &rule.template, WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WS_EX_CLIENTEDGE, (x, y, w, f.px(24.0)), Some(base + N_TEMPLATE));
    let _ = edit;
    f.y += f.px(30.0);
    // Token chips.
    let mut cx = f.ctrl_x();
    for (i, (token, _desc)) in TOKENS.iter().enumerate() {
        let bw = f.px(14.0 + 7.0 * token.len() as f64);
        if cx + bw > f.left + f.width {
            cx = f.ctrl_x();
            f.y += f.px(28.0);
        }
        let y = f.y;
        f.create(w!("BUTTON"), token, WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), Default::default(), (cx, y, bw, f.px(24.0)), Some(base + N_TOKEN + i as u16));
        cx += bw + f.px(4.0);
    }
    f.y += f.px(32.0);
    // The date and time choices are examples of the file name itself, so they stay as
    // the shared code renders them; the case and separator choices are translated words.
    f.two_combos(
        &tr("Date / time:"),
        (base + N_DATE, &labels(&DateFormat::ALL, |d| d.label()), pos(&DateFormat::ALL, rule.date)),
        (base + N_TIME, &labels(&TimeFormat::ALL, |d| d.label()), pos(&TimeFormat::ALL, rule.time)),
    );
    f.two_combos(
        &tr("Formatting:"),
        (base + N_CASE, &labels(&Case::ALL, Case::ui_label), pos(&Case::ALL, rule.case)),
        (base + N_SEP, &labels(&Separator::ALL, Separator::ui_label), pos(&Separator::ALL, rule.separator)),
    );
    f.label(&tr("Preview:"), f.px(20.0));
    let (x, w) = (f.ctrl_x(), f.ctrl_w());
    let y = f.y;
    let p = f.create(w!("STATIC"), "", WS_VISIBLE, Default::default(), (x, y + f.px(2.0), w, f.px(20.0)), None);
    f.st.previews.push((base, p));
    f.y += f.px(28.0);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xffff) as u16;
            let code = ((wp.0 >> 16) & 0xffff) as u32;
            if id >= CTRL_BASE {
                let id = id - CTRL_BASE;
                if code == EN_CHANGE {
                    text_changed(id);
                } else if code == BN_CLICKED || code == CBN_SELCHANGE {
                    util::main_async(move || changed(id));
                }
            }
            return LRESULT(0);
        }
        WM_NOTIFY => {
            let nm = unsafe { &*(lp.0 as *const NMHDR) };
            if nm.code == TCN_SELCHANGE {
                let tab = unsafe { SendMessageW(nm.hwndFrom, TCM_GETCURSEL, None, None) }.0.max(0) as usize;
                show_tab(tab);
            }
            return LRESULT(0);
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wp.0 as *mut _);
            unsafe {
                SetBkMode(dc, TRANSPARENT);
                let warn = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.warnings.iter().any(|(_, h)| h.0 as isize == lp.0)));
                if warn {
                    SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x000A6EC8));
                }
                return LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize);
            }
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

pub fn show() {
    if let Some(h) = STATE.with(|s| s.borrow().as_ref().map(|s| s.hwnd)) {
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    REGISTER.call_once(|| unsafe {
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_STANDARD_CLASSES | ICC_TAB_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: w!("PhotonSettings"),
            hbrBackground: HBRUSH(GetSysColorBrush(COLOR_WINDOW).0),
            hIcon: windows::Win32::UI::WindowsAndMessaging::LoadIconW(Some(GetModuleHandleW(None).unwrap_or_default().into()), crate::util::int_resource(1)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
    let c = hud::cursor();
    let (s, work, _) = hud::monitor_at(c);
    let (w, h) = ((640.0 * s) as i32, ((800.0 * s) as i32).min(work.h as i32 - 20));
    let title = W::new(&tr("Photon Settings"));
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            w!("PhotonSettings"),
            title.p(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            (work.mid().x - w as f64 / 2.0) as i32,
            (work.mid().y - h as f64 / 2.0) as i32,
            w,
            h,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap_or_default().into()),
            None,
        )
        .unwrap_or_default()
    };
    let s = unsafe { GetDpiForWindow(hwnd) } as f64 / 96.0;
    let font_h = -((9.0 * 96.0 * s / 72.0).round() as i32);
    let mk = |weight| unsafe {
        CreateFontW(font_h, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, FF_DONTCARE.0 as u32, w!("Segoe UI"))
    };
    let (font, bold) = (mk(FW_NORMAL.0 as i32), mk(FW_SEMIBOLD.0 as i32));
    let mut client = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
    }
    let tabs = unsafe {
        CreateWindowExW(
            Default::default(),
            WC_TABCONTROLW,
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            (8.0 * s) as i32,
            (8.0 * s) as i32,
            client.right - (16.0 * s) as i32,
            client.bottom - (16.0 * s) as i32,
            Some(hwnd),
            Some(HMENU((CTRL_BASE + TABS) as usize as *mut _)),
            Some(GetModuleHandleW(None).unwrap_or_default().into()),
            None,
        )
        .unwrap_or_default()
    };
    unsafe {
        SendMessageW(tabs, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        for (i, name) in [tr("General"), tr("Screenshots"), tr("Recordings")].iter().enumerate() {
            let mut t = util::wide(name);
            let item = TCITEMW { mask: TCIF_TEXT, pszText: windows::core::PWSTR(t.as_mut_ptr()), ..Default::default() };
            SendMessageW(tabs, TCM_INSERTITEMW, Some(WPARAM(i)), Some(LPARAM(&item as *const _ as isize)));
        }
    }
    let mut st = State {
        hwnd,
        tabs,
        ctls: Vec::new(),
        statics: Vec::new(),
        font,
        bold,
        dests: Vec::new(),
        previews: Vec::new(),
        warnings: Vec::new(),
        note: HWND::default(),
        mics: crate::record::microphones(),
        cams: crate::record::cameras(),
        recording_key: None,
        tab: 0,
    };
    build(&mut st, s, client.right);
    // Controls must sit above the tab control in z-order to receive clicks.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            tabs,
            Some(windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM),
            0,
            0,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
        );
    }
    STATE.with(|cell| *cell.borrow_mut() = Some(st));
    show_tab(0);
    refresh();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn build(st: &mut State, s: f64, client_w: i32) {
    let set = settings::get();
    let left = (28.0 * s) as i32;
    let width = client_w - left * 2;
    let top = (54.0 * s) as i32;

    // General.
    let mut f = Form { st, tab: 0, y: top, s, left, width };
    f.section(&tr("Shortcuts"));
    f.label(&tr("Screenshot:"), f.px(28.0));
    let (x, y) = (f.ctrl_x(), f.y);
    f.create(w!("BUTTON"), &set.shot_hotkey.label(), WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), Default::default(), (x, y, f.px(160.0), f.px(28.0)), Some(SHOT_KEY));
    f.y += f.px(36.0);
    f.label(&tr("Record screen:"), f.px(28.0));
    let (x, y) = (f.ctrl_x(), f.y);
    f.create(w!("BUTTON"), &set.record_hotkey.label(), WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), Default::default(), (x, y, f.px(160.0), f.px(28.0)), Some(REC_KEY));
    f.y += f.px(36.0);
    let note = f.note(&tr("Click a shortcut, then press the new keys. It needs Ctrl, Alt or Win. Esc cancels."));
    f.st.note = note;
    f.section(&tr("Startup"));
    f.checkbox(LOGIN, &tr("Launch Photon at login"), app::login_item_enabled());
    f.section(&tr("Scripting"));
    f.note("photon://capture · photon://record · photon://stop · photon://settings");

    // Screenshots.
    let mut f = Form { st: f.st, tab: 1, y: top, s, left, width };
    f.section(&tr("After a capture"));
    f.note(&tr("Copy (Ctrl+C or Enter) puts the image on the clipboard."));
    f.checkbox(SHOT_AUTOSAVE, &tr("Also save a PNG automatically"), set.shot_autosave);
    let (opts, sel) = dest_options(&set.shot_dir, Settings::default_shot_dir());
    f.combo_row(&tr("Save to:"), SHOT_DEST, &opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel);
    f.st.dests.push((SHOT_DEST, opts));
    let warn = f.note("");
    f.st.warnings.push((SHOT_DEST, warn));
    f.section(&tr("File names"));
    naming_block(&mut f, SHOT_NAMING, &set.shot_naming);

    // Recordings.
    let mut f = Form { st: f.st, tab: 2, y: top, s, left, width };
    f.section(&tr("Saving"));
    let (opts, sel) = dest_options(&set.rec_dir, Settings::default_rec_dir());
    f.combo_row(&tr("Save to:"), REC_DEST, &opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel);
    f.st.dests.push((REC_DEST, opts));
    let warn = f.note("");
    f.st.warnings.push((REC_DEST, warn));
    let also_path = set.rec_also_dir.clone().unwrap_or_else(|| {
        let folders = cloud::discover();
        folders
            .iter()
            .find(|f| f.provider == cloud::Provider::OneDrive)
            .or_else(|| folders.first())
            .map(|f| f.default_target.clone())
            .unwrap_or_else(Settings::default_rec_dir)
    });
    f.checkbox(REC_ALSO, &tr("Also copy each recording to:"), set.rec_also_copy);
    let (opts, sel) = dest_options(&also_path, Settings::default_rec_dir());
    let (x, w) = (f.ctrl_x(), f.ctrl_w());
    f.combo_at(REC_ALSO_DEST, &opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel, x, w);
    f.y += f.px(34.0);
    f.st.dests.push((REC_ALSO_DEST, opts));
    f.section(&tr("File names"));
    naming_block(&mut f, REC_NAMING, &set.rec_naming);
    f.section(&tr("Video and audio"));
    f.two_combos(
        &tr("Format:"),
        (CODEC, &[tr("H.264 (most compatible)"), tr("HEVC (smaller files)")], (set.rec_codec == Codec::Hevc) as usize),
        (FPS, &[tr("30 fps"), tr("60 fps")], (set.rec_fps == 60) as usize),
    );
    let half = (f.ctrl_w() - f.px(8.0)) / 2;
    let dx = f.ctrl_x() + half + f.px(8.0);
    f.checkbox(SYS_AUDIO, &tr("Record system audio"), set.rec_system_audio);
    let y = f.y;
    f.checkbox(MIC, &tr("Record microphone"), set.rec_mic);
    let mics: Vec<String> = std::iter::once(tr("System default")).chain(f.st.mics.iter().map(|m| m.1.clone())).collect();
    let msel = set.rec_mic_device.as_ref().and_then(|id| f.st.mics.iter().position(|m| &m.0 == id)).map(|i| i + 1).unwrap_or(0);
    let save_y = f.y;
    f.y = y - f.px(2.0);
    f.combo_at(MIC_DEVICE, &mics, msel, dx, half);
    f.y = save_y;
    let y = f.y;
    f.checkbox(WEBCAM, &tr("Webcam bubble"), set.rec_webcam);
    let cams: Vec<String> = std::iter::once(tr("System default")).chain(f.st.cams.iter().map(|m| m.1.clone())).collect();
    let csel = set.rec_webcam_device.as_ref().and_then(|id| f.st.cams.iter().position(|m| &m.0 == id)).map(|i| i + 1).unwrap_or(0);
    let save_y = f.y;
    f.y = y - f.px(2.0);
    f.combo_at(CAM_DEVICE, &cams, csel, dx, half);
    f.y = save_y;
    f.checkbox(CURSOR, &tr("Show the pointer"), set.rec_cursor);
    f.checkbox(CLICKS, &tr("Highlight clicks"), set.rec_click_rings);
    f.checkbox(COUNTDOWN, &tr("3-second countdown"), set.rec_countdown);
}

fn show_tab(tab: usize) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(st) = s.as_mut() else { return };
        st.tab = tab;
        for c in &st.ctls {
            unsafe {
                let _ = ShowWindow(c.hwnd, if c.tab == tab { SW_SHOW } else { SW_HIDE });
            }
        }
        for (h, t) in &st.statics {
            unsafe {
                let _ = ShowWindow(*h, if *t == tab { SW_SHOW } else { SW_HIDE });
            }
        }
    });
}

pub fn select_tab_for_test(tab: usize) {
    if let Some(t) = STATE.with(|s| s.borrow().as_ref().map(|st| st.tabs)) {
        unsafe {
            SendMessageW(t, windows::Win32::UI::Controls::TCM_SETCURSEL, Some(WPARAM(tab)), None);
        }
    }
    show_tab(tab);
}

pub fn close_for_test() {
    close();
}

fn close() {
    stop_recording_key();
    if let Some(st) = STATE.with(|s| s.borrow_mut().take()) {
        unsafe {
            let _ = DestroyWindow(st.hwnd);
            let _ = DeleteObject(st.font.into());
            let _ = DeleteObject(st.bold.into());
        }
        util::release_memory_soon();
    }
}

/// Route keyboard messages: shortcut capture first, then normal dialog navigation.
pub fn is_dialog_message(msg: &MSG) -> bool {
    let Some((hwnd, recording)) = STATE.with(|s| s.borrow().as_ref().map(|st| (st.hwnd, st.recording_key))) else { return false };
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return false;
    }
    if let Some(id) = recording
        && (msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN)
    {
        let key = msg.wParam.0 as u16;
        if key == keys::vk::ESCAPE {
            finish_recording_key(id, None);
            return true;
        }
        let down = |k: i32| unsafe { GetKeyState(k) } < 0;
        let mut mods = 0;
        if down(VK_CONTROL.0 as i32) { mods |= keys::CONTROL; }
        if down(VK_SHIFT.0 as i32) { mods |= keys::SHIFT; }
        if down(VK_MENU.0 as i32) { mods |= keys::ALT; }
        if down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32) { mods |= keys::WIN; }
        let sc = Shortcut { key, mods };
        if sc.is_valid() {
            finish_recording_key(id, Some(sc));
        } else if !matches!(key, keys::vk::SHIFT | keys::vk::CONTROL | keys::vk::MENU | keys::vk::LWIN | keys::vk::RWIN) {
            set_note(&tr("Add Ctrl, Alt or Win to the key. Esc cancels."));
        }
        return true;
    }
    unsafe { IsDialogMessageW(hwnd, msg).as_bool() }
}

fn set_note(t: &str) {
    if let Some(h) = STATE.with(|s| s.borrow().as_ref().map(|st| st.note)) {
        set_text(h, t);
    }
}

fn start_recording_key(id: u16) {
    hotkey::suspend();
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.recording_key = Some(id);
        }
    });
    if let Some(h) = ctl(id) {
        set_text(h, &tr("Press shortcut…"));
        unsafe {
            let _ = SetFocus(Some(h));
        }
    }
}

fn stop_recording_key() {
    let was = STATE.with(|s| s.borrow_mut().as_mut().and_then(|st| st.recording_key.take()));
    if was.is_some() {
        hotkey::resume();
    }
}

fn finish_recording_key(id: u16, sc: Option<Shortcut>) {
    stop_recording_key();
    if let Some(sc) = sc {
        settings::update(|s| if id == SHOT_KEY { s.shot_hotkey = sc } else { s.record_hotkey = sc });
        let errors = app::apply_hotkeys();
        set_note(&if errors.is_empty() { tr("Saved.") } else { errors.join(" ") });
    }
    let s = settings::get();
    if let Some(h) = ctl(id) {
        set_text(h, &if id == SHOT_KEY { s.shot_hotkey } else { s.record_hotkey }.label());
    }
}

fn sample_parts(kind: Kind) -> NameParts {
    NameParts { kind, app: Some("Chrome".into()), size: Some((1920, 1080)), display: Some("DISPLAY1".into()) }
}

fn refresh() {
    let s = settings::get();
    STATE.with(|st| {
        let st = st.borrow();
        let Some(st) = st.as_ref() else { return };
        for (base, h) in &st.previews {
            let text = if *base == SHOT_NAMING {
                destination::file_name_preview(&s.shot_naming, s.shot_counter, &sample_parts(Kind::Screenshot), "png")
            } else {
                destination::file_name_preview(&s.rec_naming, s.rec_counter, &sample_parts(Kind::Recording), "mp4")
            };
            set_text(*h, &text);
        }
        for (id, h) in &st.warnings {
            let dir = if *id == SHOT_DEST { &s.shot_dir } else { &s.rec_dir };
            let msg = if destination::is_usable(dir) {
                String::new()
            } else {
                let fb = if *id == SHOT_DEST { Settings::default_shot_dir() } else { Settings::default_rec_dir() };
                trf("This folder isn't available. Files go to {folder} until it's back.", &[("folder", &util::tilde(&fb))])
            };
            set_text(*h, &msg);
        }
    });
    let enable = |id: u16, on: bool| {
        if let Some(h) = ctl(id) {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(h, on);
            }
        }
    };
    enable(SHOT_DEST, s.shot_autosave);
    enable(REC_ALSO_DEST, s.rec_also_copy);
    enable(MIC_DEVICE, s.rec_mic);
    enable(CAM_DEVICE, s.rec_webcam);
}

fn pick_folder(start: &std::path::Path) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().unwrap_or_default();
        let _ = dlg.SetOptions(opts | FOS_PICKFOLDERS);
        let d = W::new(&start.to_string_lossy());
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(d.p(), None) {
            let _ = dlg.SetFolder(&item);
        }
        let owner = STATE.with(|s| s.borrow().as_ref().map(|st| st.hwnd));
        dlg.Show(owner).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

fn pick_dest(id: u16, current: PathBuf) -> Option<PathBuf> {
    let i = selected(id);
    let choice = STATE.with(|s| s.borrow().as_ref().and_then(|st| st.dests.iter().find(|(d, _)| *d == id)).and_then(|(_, o)| o.get(i)).map(|o| o.path.clone()));
    match choice? {
        Some(p) => Some(p),
        None => {
            let picked = pick_folder(&current);
            if picked.is_none() {
                rebuild_dest(id, &current, if id == SHOT_DEST { Settings::default_shot_dir() } else { Settings::default_rec_dir() });
            }
            picked
        }
    }
}

fn rebuild_dest(id: u16, current: &std::path::Path, default: PathBuf) {
    let (opts, sel) = dest_options(current, default);
    if let Some(h) = ctl(id) {
        fill_combo(h, &opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel);
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.dests.retain(|(d, _)| *d != id);
            st.dests.push((id, opts));
        }
    });
}

fn text_changed(id: u16) {
    let Some(h) = ctl(id) else { return };
    let t = text_of(h);
    match id {
        x if x == SHOT_NAMING + N_TEMPLATE => {
            settings::update(|s| s.shot_naming.template = t);
        }
        x if x == REC_NAMING + N_TEMPLATE => {
            settings::update(|s| s.rec_naming.template = t);
        }
        _ => return,
    }
    refresh();
}

fn naming_changed(base: u16, offset: u16) {
    fn rule(s: &mut Settings, base: u16) -> &mut NamingRule {
        if base == SHOT_NAMING { &mut s.shot_naming } else { &mut s.rec_naming }
    }
    if offset >= N_TOKEN {
        let Some((token, _)) = TOKENS.get((offset - N_TOKEN) as usize) else { return };
        if let Some(h) = ctl(base + N_TEMPLATE) {
            let mut t = text_of(h);
            if !t.is_empty() && !t.ends_with(' ') {
                t.push(' ');
            }
            t.push_str(token);
            set_text(h, &t); // triggers EN_CHANGE, which saves
        }
        return;
    }
    let i = selected(base + offset);
    settings::update(|s| {
        let r = rule(s, base);
        match offset {
            N_DATE => r.date = DateFormat::ALL[i.min(DateFormat::ALL.len() - 1)],
            N_TIME => r.time = TimeFormat::ALL[i.min(TimeFormat::ALL.len() - 1)],
            N_CASE => r.case = Case::ALL[i.min(Case::ALL.len() - 1)],
            N_SEP => r.separator = Separator::ALL[i.min(Separator::ALL.len() - 1)],
            _ => {}
        }
    });
}

/// A control changed (from WM_COMMAND).
pub fn changed(id: u16) {
    let s = settings::get();
    match id {
        SHOT_KEY | REC_KEY => start_recording_key(id),
        LOGIN => {
            app::toggle_login_item();
            if let Some(h) = ctl(LOGIN) {
                set_check(h, app::login_item_enabled());
            }
        }
        SHOT_AUTOSAVE => {
            let on = is_on(id);
            settings::update(|s| s.shot_autosave = on);
        }
        SHOT_DEST => {
            if let Some(p) = pick_dest(id, s.shot_dir.clone()) {
                settings::update(|s| s.shot_dir = p.clone());
                rebuild_dest(id, &p, Settings::default_shot_dir());
            }
        }
        REC_DEST => {
            if let Some(p) = pick_dest(id, s.rec_dir.clone()) {
                settings::update(|s| s.rec_dir = p.clone());
                rebuild_dest(id, &p, Settings::default_rec_dir());
            }
        }
        REC_ALSO => {
            let on = is_on(id);
            let i = selected(REC_ALSO_DEST);
            let path = STATE.with(|st| st.borrow().as_ref().and_then(|st| st.dests.iter().find(|(d, _)| *d == REC_ALSO_DEST)).and_then(|(_, o)| o.get(i)).and_then(|o| o.path.clone()));
            settings::update(|s| {
                s.rec_also_copy = on;
                if s.rec_also_dir.is_none() {
                    s.rec_also_dir = path;
                }
            });
        }
        REC_ALSO_DEST => {
            let cur = s.rec_also_dir.clone().unwrap_or_else(Settings::default_rec_dir);
            if let Some(p) = pick_dest(id, cur) {
                settings::update(|s| s.rec_also_dir = Some(p.clone()));
                rebuild_dest(id, &p, Settings::default_rec_dir());
            }
        }
        CODEC => {
            let hevc = selected(id) == 1;
            settings::update(|s| s.rec_codec = if hevc { Codec::Hevc } else { Codec::H264 });
        }
        FPS => {
            let sixty = selected(id) == 1;
            settings::update(|s| s.rec_fps = if sixty { 60 } else { 30 });
        }
        SYS_AUDIO => {
            let on = is_on(id);
            settings::update(|s| s.rec_system_audio = on);
        }
        MIC => {
            let on = is_on(id);
            settings::update(|s| s.rec_mic = on);
        }
        WEBCAM => {
            let on = is_on(id);
            settings::update(|s| s.rec_webcam = on);
        }
        MIC_DEVICE | CAM_DEVICE => {
            let i = selected(id);
            let dev = STATE.with(|st| {
                let st = st.borrow();
                let st = st.as_ref()?;
                let list = if id == MIC_DEVICE { &st.mics } else { &st.cams };
                i.checked_sub(1).and_then(|i| list.get(i)).map(|d| d.0.clone())
            });
            settings::update(|s| if id == MIC_DEVICE { s.rec_mic_device = dev } else { s.rec_webcam_device = dev });
        }
        CURSOR => {
            let on = is_on(id);
            settings::update(|s| s.rec_cursor = on);
        }
        CLICKS => {
            let on = is_on(id);
            settings::update(|s| s.rec_click_rings = on);
        }
        COUNTDOWN => {
            let on = is_on(id);
            settings::update(|s| s.rec_countdown = on);
        }
        x if (SHOT_NAMING..SHOT_NAMING + 50).contains(&x) => naming_changed(SHOT_NAMING, x - SHOT_NAMING),
        x if (REC_NAMING..REC_NAMING + 50).contains(&x) => naming_changed(REC_NAMING, x - REC_NAMING),
        _ => {}
    }
    refresh();
}
