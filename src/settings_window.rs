//! The Settings window: General, Screenshots and Recordings tabs. Every control
//! carries `Command::Setting(id)` and writes straight to settings on change.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezelStyle, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn,
    NSEvent, NSEventMask, NSFont, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSTabView, NSTabViewItem,
    NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask, NSFontWeightSemibold,
};
use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo};
use objc2_foundation::{NSNotification, NSObjectProtocol, NSString};

use crate::capture::cg_rect;
use crate::commands::Command;
use crate::geometry::Rect;
use crate::i18n::{tr, trf};
use crate::keys::{self, vk, Shortcut};
use crate::output::cloud;
use crate::output::destination::{self, NameParts};
use crate::output::naming::{Case, DateFormat, Kind, NamingRule, Separator, TimeFormat, TOKENS};
use crate::settings::{self, Codec, Settings};
use crate::util::{self, ns};
use crate::{app, hotkey, ui};

// Control ids.
const SHOT_KEY: u16 = 1;
const REC_KEY: u16 = 2;
const LOGIN: u16 = 3;
const UPDATES: u16 = 4;
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
// Naming blocks: base + offset. Screenshots 100.., recordings 200..
const N_TEMPLATE: u16 = 0;
const N_DATE: u16 = 1;
const N_TIME: u16 = 2;
const N_CASE: u16 = 3;
const N_SEP: u16 = 4;
const N_TOKEN: u16 = 10; // + token index
const SHOT_NAMING: u16 = 100;
const REC_NAMING: u16 = 200;

define_class!(
    // SAFETY: NSView subclass that only flips its coordinate system.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonFlippedView"]
    struct Flipped;

    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    // SAFETY: NSObject subclass acting as the window delegate; no Drop impl.
    #[unsafe(super = objc2_foundation::NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "PhotonSettingsDelegate"]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            util::main_async(closed);
        }
    }
);

struct DestOption {
    label: String,
    /// None = "Other Folder…".
    path: Option<PathBuf>,
}

struct State {
    window: Retained<NSWindow>,
    _delegate: Retained<WindowDelegate>,
    controls: HashMap<u16, Retained<NSView>>,
    dests: HashMap<u16, Vec<DestOption>>,
    previews: HashMap<u16, Retained<NSTextField>>,
    warnings: HashMap<u16, Retained<NSTextField>>,
    mics: Vec<(String, String)>,
    cams: Vec<(String, String)>,
    recording_key: Option<(u16, Retained<AnyObject>)>,
    hotkey_note: Retained<NSTextField>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().unwrap()
}

/// Top-down layout inside a flipped container.
struct Form {
    view: Retained<Flipped>,
    y: f64,
    width: f64,
    controls: Vec<(u16, Retained<NSView>)>,
}

const LABEL_W: f64 = 150.0;
const X: f64 = 24.0;

impl Form {
    fn new(width: f64, height: f64) -> Form {
        let this = Flipped::alloc(mtm()).set_ivars(());
        let view: Retained<Flipped> = unsafe { msg_send![super(this), initWithFrame: cg_rect(Rect::new(0.0, 0.0, width, height))] };
        Form { view, y: 18.0, width, controls: Vec::new() }
    }

    fn label(&self, text: &str, y: f64, h: f64) {
        let l = NSTextField::labelWithString(&ns(text), mtm());
        l.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        l.setFrame(cg_rect(Rect::new(X, y + (h - 17.0) / 2.0, LABEL_W, 17.0)));
        self.view.addSubview(&l);
    }

    fn section(&mut self, title: &str) {
        if self.y > 20.0 {
            self.y += 10.0;
        }
        let l = NSTextField::labelWithString(&ns(title), mtm());
        l.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe { NSFontWeightSemibold })));
        l.setFrame(cg_rect(Rect::new(X, self.y, self.width - 2.0 * X, 18.0)));
        self.view.addSubview(&l);
        self.y += 26.0;
    }

    /// A labelled row. `w` is the control width (None = rest of the row).
    fn row(&mut self, label: &str, id: u16, control: Retained<NSView>, w: Option<f64>, h: f64) {
        self.label(label, self.y, h);
        let cx = X + LABEL_W + 10.0;
        let cw = w.unwrap_or(self.width - cx - X);
        control.setFrame(cg_rect(Rect::new(cx, self.y, cw, h)));
        self.view.addSubview(&control);
        self.controls.push((id, control));
        self.y += h + 8.0;
    }

    /// A control without a label, aligned with the control column.
    fn plain(&mut self, id: u16, control: Retained<NSView>, h: f64) {
        let cx = X + LABEL_W + 10.0;
        control.setFrame(cg_rect(Rect::new(cx, self.y, self.width - cx - X, h)));
        self.view.addSubview(&control);
        if id != 0 {
            self.controls.push((id, control));
        }
        self.y += h + 6.0;
    }

    fn note(&mut self, text: &str) -> Retained<NSTextField> {
        let l = NSTextField::wrappingLabelWithString(&ns(text), mtm());
        l.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        l.setTextColor(Some(&NSColor::secondaryLabelColor()));
        let cx = X + LABEL_W + 10.0;
        l.setFrame(cg_rect(Rect::new(cx, self.y - 4.0, self.width - cx - X, 30.0)));
        self.view.addSubview(&l);
        self.y += 24.0;
        l
    }
}

fn target_action(c: &objc2_app_kit::NSControl, id: u16) {
    let t = ui::controller(mtm());
    unsafe {
        c.setTarget(Some(&t));
        c.setAction(Some(sel!(command:)));
    }
    c.setTag(Command::Setting(id).tag());
}

fn checkbox(title: &str, id: u16, on: bool) -> Retained<NSView> {
    let t = ui::controller(mtm());
    let b = unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), Some(&t), Some(sel!(command:)), mtm()) };
    b.setTag(Command::Setting(id).tag());
    b.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
    Retained::into_super(Retained::into_super(b))
}

fn popup(items: &[String], selected: usize, id: u16) -> Retained<NSView> {
    let p = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm()), cg_rect(Rect::new(0.0, 0.0, 200.0, 25.0)), false);
    for i in items {
        p.addItemWithTitle(&ns(i));
    }
    p.selectItemAtIndex(selected.min(items.len().saturating_sub(1)) as isize);
    target_action(&p, id);
    Retained::into_super(Retained::into_super(Retained::into_super(p)))
}

fn shortcut_button(sc: Shortcut, id: u16) -> Retained<NSView> {
    let b = ui::text_button(mtm(), &sc.label(), Command::Setting(id));
    b.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(13.0, unsafe { objc2_app_kit::NSFontWeightMedium })));
    Retained::into_super(Retained::into_super(b))
}

fn dest_options(current: &std::path::Path, default: PathBuf) -> (Vec<DestOption>, usize) {
    let folders = cloud::discover();
    let mut opts = vec![DestOption {
        label: trf("{folder}  (default)", &[("folder", &util::tilde(&default))]),
        path: Some(default.clone()),
    }];
    for f in &folders {
        let label = if f.stale { trf("{folder}  (old)", &[("folder", &f.label)]) } else { f.label.clone() };
        opts.push(DestOption { label: format!("{label} › Photon"), path: Some(f.default_target.clone()) });
    }
    let mut selected = opts.iter().position(|o| o.path.as_deref() == Some(current));
    if selected.is_none() {
        opts.insert(1, DestOption { label: cloud::display_path(current, &folders), path: Some(current.to_path_buf()) });
        selected = Some(1);
    }
    opts.push(DestOption { label: tr("Other Folder…"), path: None });
    (opts, selected.unwrap_or(0))
}

fn pos<T: PartialEq>(all: &[T], v: T) -> usize {
    all.iter().position(|x| *x == v).unwrap_or(0)
}

fn naming_block(f: &mut Form, base: u16, rule: &NamingRule) {
    let field = NSTextField::textFieldWithString(&ns(&rule.template), mtm());
    field.setTag(Command::Setting(base + N_TEMPLATE).tag());
    unsafe { field.setDelegate(Some(&ui::text_delegate(mtm()))) };
    f.row(&tr("File name:"), base + N_TEMPLATE, Retained::into_super(Retained::into_super(field)), None, 22.0);

    // Token chips.
    let cx = X + LABEL_W + 10.0;
    let mut x = cx;
    for (i, (token, desc)) in TOKENS.iter().enumerate() {
        let b = ui::text_button(mtm(), token, Command::Setting(base + N_TOKEN + i as u16));
        b.setBezelStyle(NSBezelStyle::Badge);
        b.setFont(Some(&NSFont::systemFontOfSize(10.5)));
        b.setToolTip(Some(&ns(&desc())));
        let w = 14.0 + 6.2 * token.len() as f64;
        if x + w > f.width - X {
            x = cx;
            f.y += 22.0;
        }
        b.setFrame(cg_rect(Rect::new(x, f.y - 2.0, w, 20.0)));
        f.view.addSubview(&b);
        x += w + 4.0;
    }
    f.y += 26.0;

    let labels = |v: Vec<&'static str>| v.into_iter().map(String::from).collect::<Vec<_>>();
    let row_y = f.y;
    let half = (f.width - (X + LABEL_W + 10.0) - X - 8.0) / 2.0;
    f.row(&tr("Date / time:"), base + N_DATE, popup(&labels(DateFormat::ALL.iter().map(|d| d.label()).collect()), pos(&DateFormat::ALL, rule.date), base + N_DATE), Some(half), 25.0);
    let time = popup(&labels(TimeFormat::ALL.iter().map(|d| d.label()).collect()), pos(&TimeFormat::ALL, rule.time), base + N_TIME);
    time.setFrame(cg_rect(Rect::new(X + LABEL_W + 10.0 + half + 8.0, row_y, half, 25.0)));
    f.view.addSubview(&time);
    f.controls.push((base + N_TIME, time));
    let row_y = f.y;
    f.row(&tr("Formatting:"), base + N_CASE, popup(&Case::ALL.map(Case::ui_label), pos(&Case::ALL, rule.case), base + N_CASE), Some(half), 25.0);
    let sep = popup(&Separator::ALL.map(Separator::ui_label), pos(&Separator::ALL, rule.separator), base + N_SEP);
    sep.setFrame(cg_rect(Rect::new(X + LABEL_W + 10.0 + half + 8.0, row_y, half, 25.0)));
    f.view.addSubview(&sep);
    f.controls.push((base + N_SEP, sep));
}

fn preview_label(f: &mut Form) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&ns(""), mtm());
    l.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(11.0, unsafe { objc2_app_kit::NSFontWeightRegular })));
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingMiddle);
    f.label(&tr("Preview:"), f.y, 17.0);
    let cx = X + LABEL_W + 10.0;
    l.setFrame(cg_rect(Rect::new(cx, f.y, f.width - cx - X, 17.0)));
    f.view.addSubview(&l);
    f.y += 24.0;
    l
}

fn warning_label(f: &mut Form) -> Retained<NSTextField> {
    let l = f.note("");
    l.setTextColor(Some(&NSColor::systemOrangeColor()));
    l
}

fn devices(video: bool) -> Vec<(String, String)> {
    let ty = unsafe { if video { AVMediaTypeVideo } else { AVMediaTypeAudio } };
    let Some(ty) = ty else { return Vec::new() };
    #[allow(deprecated)]
    let list = unsafe { AVCaptureDevice::devicesWithMediaType(ty) };
    list.iter().map(|d| unsafe { (d.uniqueID().to_string(), d.localizedName().to_string()) }).collect()
}

fn device_popup(list: &[(String, String)], current: Option<&str>, id: u16) -> Retained<NSView> {
    let mut items = vec![tr("System default")];
    items.extend(list.iter().map(|(_, n)| n.clone()));
    let sel = current.and_then(|c| list.iter().position(|(i, _)| i == c)).map(|i| i + 1).unwrap_or(0);
    popup(&items, sel, id)
}

pub fn show() {
    let mtm = mtm();
    if let Some(w) = STATE.with(|s| s.borrow().as_ref().map(|s| s.window.clone())) {
        ui::app(mtm).activate();
        w.makeKeyAndOrderFront(None);
        return;
    }
    let s = settings::get();
    let (w, h) = (600.0, 640.0);
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            cg_rect(Rect::new(0.0, 0.0, w, h)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&ns(&tr("Photon Settings")));
    let tabs = NSTabView::initWithFrame(NSTabView::alloc(mtm), cg_rect(Rect::new(0.0, 0.0, w, h)));
    let inner_w = w - 20.0;
    let inner_h = h - 40.0;
    let mut controls: HashMap<u16, Retained<NSView>> = HashMap::new();
    let mut dests = HashMap::new();
    let mut previews = HashMap::new();
    let mut warnings = HashMap::new();

    // General.
    let mut g = Form::new(inner_w, inner_h);
    g.section(&tr("Shortcuts"));
    g.row(&tr("Screenshot:"), SHOT_KEY, shortcut_button(s.shot_hotkey, SHOT_KEY), Some(160.0), 28.0);
    g.row(&tr("Record screen:"), REC_KEY, shortcut_button(s.record_hotkey, REC_KEY), Some(160.0), 28.0);
    let hotkey_note = g.note(&tr("Click a shortcut, then press the new keys. It needs ⌘, ⌃ or ⌥. Esc cancels."));
    g.section(&tr("Startup"));
    g.plain(LOGIN, checkbox(&tr("Launch Photon at login"), LOGIN, app::login_item_enabled()), 20.0);
    g.section(&tr("Updates"));
    g.plain(UPDATES, checkbox(&tr("Check for updates automatically"), UPDATES, s.check_updates), 20.0);
    g.note(&tr("Once a day, Photon asks GitHub whether there's a newer version."));
    g.section(&tr("Scripting"));
    g.note("open photon://capture · photon://record · photon://stop · photon://settings");

    // Screenshots.
    let mut sh = Form::new(inner_w, inner_h);
    sh.section(&tr("After a capture"));
    sh.plain(0, Retained::into_super(Retained::into_super(NSTextField::labelWithString(&ns(&tr("Copy (⌘C or ↩) puts the image on the clipboard.")), mtm))), 17.0);
    sh.plain(SHOT_AUTOSAVE, checkbox(&tr("Also save a PNG automatically"), SHOT_AUTOSAVE, s.shot_autosave), 20.0);
    let (opts, sel) = dest_options(&s.shot_dir, Settings::default_shot_dir());
    sh.row(&tr("Save to:"), SHOT_DEST, popup(&opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel, SHOT_DEST), None, 25.0);
    dests.insert(SHOT_DEST, opts);
    warnings.insert(SHOT_DEST, warning_label(&mut sh));
    sh.section(&tr("File names"));
    naming_block(&mut sh, SHOT_NAMING, &s.shot_naming);
    previews.insert(SHOT_NAMING, preview_label(&mut sh));

    // Recordings.
    let mut rc = Form::new(inner_w, inner_h);
    rc.section(&tr("Saving"));
    let (opts, sel) = dest_options(&s.rec_dir, Settings::default_rec_dir());
    rc.row(&tr("Save to:"), REC_DEST, popup(&opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel, REC_DEST), None, 25.0);
    dests.insert(REC_DEST, opts);
    warnings.insert(REC_DEST, warning_label(&mut rc));
    let also_path = s.rec_also_dir.clone().unwrap_or_else(|| {
        let folders = cloud::discover();
        folders
            .iter()
            .find(|f| !f.stale && f.provider == cloud::Provider::OneDrive)
            .or_else(|| folders.iter().find(|f| !f.stale))
            .map(|f| f.default_target.clone())
            .unwrap_or_else(Settings::default_rec_dir)
    });
    rc.plain(REC_ALSO, checkbox(&tr("Also copy each recording to:"), REC_ALSO, s.rec_also_copy), 20.0);
    let (opts, sel) = dest_options(&also_path, Settings::default_rec_dir());
    rc.plain(REC_ALSO_DEST, popup(&opts.iter().map(|o| o.label.clone()).collect::<Vec<_>>(), sel, REC_ALSO_DEST), 25.0);
    dests.insert(REC_ALSO_DEST, opts);
    rc.section(&tr("File names"));
    naming_block(&mut rc, REC_NAMING, &s.rec_naming);
    previews.insert(REC_NAMING, preview_label(&mut rc));
    rc.section(&tr("Video and audio"));
    let half = (inner_w - (X + LABEL_W + 10.0) - X - 8.0) / 2.0;
    let y = rc.y;
    // Codec names are long and frame rates short, so the codec popup takes more of the row.
    // The right edge still lines up with the device popups below.
    let fps_w = 120.0;
    let codec_w = half * 2.0 - fps_w;
    rc.row(&tr("Format:"), CODEC, popup(&[tr("H.264 (most compatible)"), tr("HEVC (smaller files)")], (s.rec_codec == Codec::Hevc) as usize, CODEC), Some(codec_w), 25.0);
    let fps = popup(&[tr("30 fps"), tr("60 fps")], (s.rec_fps == 60) as usize, FPS);
    fps.setFrame(cg_rect(Rect::new(X + LABEL_W + 10.0 + codec_w + 8.0, y, fps_w, 25.0)));
    rc.view.addSubview(&fps);
    rc.controls.push((FPS, fps));
    let mics = devices(false);
    let cams = devices(true);
    rc.plain(SYS_AUDIO, checkbox(&tr("Record system audio"), SYS_AUDIO, s.rec_system_audio), 20.0);
    let y = rc.y;
    rc.plain(MIC, checkbox(&tr("Record microphone"), MIC, s.rec_mic), 20.0);
    let micp = device_popup(&mics, s.rec_mic_device.as_deref(), MIC_DEVICE);
    micp.setFrame(cg_rect(Rect::new(X + LABEL_W + 10.0 + half + 8.0, y - 3.0, half, 25.0)));
    rc.view.addSubview(&micp);
    rc.controls.push((MIC_DEVICE, micp));
    let y = rc.y;
    rc.plain(WEBCAM, checkbox(&tr("Webcam bubble"), WEBCAM, s.rec_webcam), 20.0);
    let camp = device_popup(&cams, s.rec_webcam_device.as_deref(), CAM_DEVICE);
    camp.setFrame(cg_rect(Rect::new(X + LABEL_W + 10.0 + half + 8.0, y - 3.0, half, 25.0)));
    rc.view.addSubview(&camp);
    rc.controls.push((CAM_DEVICE, camp));
    rc.plain(CURSOR, checkbox(&tr("Show the pointer"), CURSOR, s.rec_cursor), 20.0);
    rc.plain(CLICKS, checkbox(&tr("Highlight clicks"), CLICKS, s.rec_click_rings), 20.0);
    rc.plain(COUNTDOWN, checkbox(&tr("3-second countdown"), COUNTDOWN, s.rec_countdown), 20.0);

    let tab_list = [("General", tr("General"), g), ("Screenshots", tr("Screenshots"), sh), ("Recordings", tr("Recordings"), rc)];
    for (id, title, form) in tab_list {
        let item = unsafe { NSTabViewItem::initWithIdentifier(NSTabViewItem::alloc(), Some(&ns(id))) };
        item.setLabel(&ns(&title));
        item.setView(Some(&form.view));
        tabs.addTabViewItem(&item);
        controls.extend(form.controls);
    }
    window.setContentView(Some(&tabs));
    let this = WindowDelegate::alloc(mtm).set_ivars(());
    let delegate: Retained<WindowDelegate> = unsafe { msg_send![super(this), init] };
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    window.center();

    STATE.with(|st| {
        *st.borrow_mut() = Some(State {
            window: window.clone(),
            _delegate: delegate,
            controls,
            dests,
            previews,
            warnings,
            mics,
            cams,
            recording_key: None,
            hotkey_note,
        })
    });
    refresh();
    ui::app(mtm).activate();
    window.makeKeyAndOrderFront(None);
}

/// Developer self-test: switch tabs.
pub fn select_tab(i: isize) {
    STATE.with(|s| {
        if let Some(s) = s.borrow().as_ref()
            && let Some(v) = s.window.contentView()
                && let Ok(tabs) = v.downcast::<NSTabView>() {
                    tabs.selectTabViewItemAtIndex(i);
                }
    });
}

pub fn move_for_test() {
    if let Some(w) = STATE.with(|s| s.borrow().as_ref().map(|s| s.window.clone())) {
        w.setFrameOrigin(objc2_foundation::NSPoint::new(30.0, 80.0));
    }
}

pub fn window_number_for_test() -> Option<isize> {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.window.windowNumber()))
}

/// The Settings window, for self-tests that draw it into an image.
pub fn window_for_test() -> Option<objc2::rc::Retained<NSWindow>> {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.window.clone()))
}

pub fn close_for_test() {
    let w = STATE.with(|s| s.borrow().as_ref().map(|s| s.window.clone()));
    if let Some(w) = w {
        w.close();
    }
}

fn closed() {
    stop_recording_key();
    STATE.with(|s| s.borrow_mut().take());
    ui::app(mtm()).deactivate();
    util::release_memory_soon();
}

fn control<T: objc2::Message>(id: u16) -> Option<Retained<T>> {
    STATE.with(|s| {
        let s = s.borrow();
        let v = s.as_ref()?.controls.get(&id)?.clone();
        // SAFETY: each id is registered with the control type the caller asks for.
        Some(unsafe { Retained::cast_unchecked::<T>(v) })
    })
}

fn is_on(id: u16) -> bool {
    control::<NSButton>(id).map(|b| b.state() == NSControlStateValueOn).unwrap_or(false)
}

fn selected(id: u16) -> usize {
    control::<NSPopUpButton>(id).map(|p| p.indexOfSelectedItem().max(0) as usize).unwrap_or(0)
}

fn sample_parts(kind: Kind) -> NameParts {
    NameParts { kind, app: Some("Safari".into()), size: Some((1440, 900)), display: Some(tr("Built-in Display")) }
}

/// Update previews and warnings from current settings.
fn refresh() {
    let s = settings::get();
    STATE.with(|st| {
        let st = st.borrow();
        let Some(st) = st.as_ref() else { return };
        if let Some(p) = st.previews.get(&SHOT_NAMING) {
            p.setStringValue(&ns(&destination::file_name_preview(&s.shot_naming, s.shot_counter, &sample_parts(Kind::Screenshot), "png")));
        }
        if let Some(p) = st.previews.get(&REC_NAMING) {
            p.setStringValue(&ns(&destination::file_name_preview(&s.rec_naming, s.rec_counter, &sample_parts(Kind::Recording), "mp4")));
        }
        for (id, dir) in [(SHOT_DEST, &s.shot_dir), (REC_DEST, &s.rec_dir)] {
            if let Some(w) = st.warnings.get(&id) {
                let msg = if destination::is_usable(dir) {
                    String::new()
                } else {
                    let fallback = if id == SHOT_DEST { Settings::default_shot_dir() } else { Settings::default_rec_dir() };
                    trf(
                        "This folder isn't available (signed out?). Files go to {folder} until it's back.",
                        &[("folder", &util::tilde(&fallback))],
                    )
                };
                w.setStringValue(&ns(&msg));
            }
        }
    });
    if let Some(p) = control::<NSPopUpButton>(SHOT_DEST) {
        p.setEnabled(s.shot_autosave);
    }
    if let Some(p) = control::<NSPopUpButton>(REC_ALSO_DEST) {
        p.setEnabled(s.rec_also_copy);
    }
    if let Some(p) = control::<NSPopUpButton>(MIC_DEVICE) {
        p.setEnabled(s.rec_mic);
    }
    if let Some(p) = control::<NSPopUpButton>(CAM_DEVICE) {
        p.setEnabled(s.rec_webcam);
    }
}

fn choose_folder(start: &std::path::Path) -> Option<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm());
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setCanCreateDirectories(true);
    panel.setPrompt(Some(&ns(&tr("Choose"))));
    panel.setDirectoryURL(Some(&objc2_foundation::NSURL::fileURLWithPath(&ns(&start.to_string_lossy()))));
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    panel.URLs().firstObject().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()))
}

fn pick_dest(id: u16, current: PathBuf) -> Option<PathBuf> {
    let i = selected(id);
    let choice = STATE.with(|s| s.borrow().as_ref().and_then(|s| s.dests.get(&id)).and_then(|d| d.get(i)).map(|o| o.path.clone()));
    match choice? {
        Some(p) => Some(p),
        None => {
            let picked = choose_folder(&current);
            if picked.is_none() {
                // Cancelled: put the popup back on the current folder.
                let idx = STATE.with(|s| {
                    s.borrow().as_ref().and_then(|s| s.dests.get(&id)).and_then(|d| d.iter().position(|o| o.path.as_deref() == Some(&*current)))
                });
                if let (Some(p), Some(idx)) = (control::<NSPopUpButton>(id), idx) {
                    p.selectItemAtIndex(idx as isize);
                }
            }
            picked
        }
    }
}

fn rebuild_dest_popup(id: u16, current: &std::path::Path, default: PathBuf) {
    let (opts, sel) = dest_options(current, default);
    if let Some(p) = control::<NSPopUpButton>(id) {
        p.removeAllItems();
        for o in &opts {
            p.addItemWithTitle(&ns(&o.label));
        }
        p.selectItemAtIndex(sel as isize);
    }
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.dests.insert(id, opts);
        }
    });
}

fn naming_changed(base: u16, offset: u16) {
    fn rule(s: &mut Settings, base: u16) -> &mut NamingRule {
        if base == SHOT_NAMING { &mut s.shot_naming } else { &mut s.rec_naming }
    }
    if offset >= N_TOKEN {
        let Some((token, _)) = TOKENS.get((offset - N_TOKEN) as usize) else { return };
        if let Some(field) = control::<NSTextField>(base + N_TEMPLATE) {
            let mut t = field.stringValue().to_string();
            if !t.is_empty() && !t.ends_with(' ') {
                t.push(' ');
            }
            t.push_str(token);
            field.setStringValue(&ns(&t));
            settings::update(|s| rule(s, base).template = t);
        }
    } else {
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
    refresh();
}

/// Template text field edits (from the shared text delegate). Returns true when handled.
pub fn text_changed(obj: Option<&AnyObject>) -> bool {
    let Some(field) = obj.and_then(|o| o.downcast_ref::<NSTextField>()) else { return false };
    let Some(Command::Setting(id)) = Command::from_tag(field.tag()) else { return false };
    let text = field.stringValue().to_string();
    match id {
        x if x == SHOT_NAMING + N_TEMPLATE => {
            settings::update(|s| s.shot_naming.template = text);
        }
        x if x == REC_NAMING + N_TEMPLATE => {
            settings::update(|s| s.rec_naming.template = text);
        }
        _ => return false,
    }
    refresh();
    true
}

/// A control changed.
pub fn changed(id: u16) {
    let s = settings::get();
    match id {
        SHOT_KEY | REC_KEY => start_recording_key(id),
        LOGIN => app::toggle_login_item(),
        UPDATES => {
            let on = is_on(id);
            settings::update(|s| s.check_updates = on);
            crate::update_ui::setting_changed();
        }
        SHOT_AUTOSAVE => {
            let on = is_on(id);
            settings::update(|s| s.shot_autosave = on);
        }
        SHOT_DEST => {
            if let Some(p) = pick_dest(id, s.shot_dir.clone()) {
                settings::update(|s| s.shot_dir = p.clone());
                rebuild_dest_popup(id, &p, Settings::default_shot_dir());
            }
        }
        REC_DEST => {
            if let Some(p) = pick_dest(id, s.rec_dir.clone()) {
                settings::update(|s| s.rec_dir = p.clone());
                rebuild_dest_popup(id, &p, Settings::default_rec_dir());
            }
        }
        REC_ALSO => {
            let on = is_on(id);
            let i = selected(REC_ALSO_DEST);
            let path = STATE.with(|st| {
                st.borrow().as_ref().and_then(|st| st.dests.get(&REC_ALSO_DEST)).and_then(|d| d.get(i)).and_then(|o| o.path.clone())
            });
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
                rebuild_dest_popup(id, &p, Settings::default_rec_dir());
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
            if on {
                crate::permissions::ask_microphone();
            }
        }
        WEBCAM => {
            let on = is_on(id);
            settings::update(|s| s.rec_webcam = on);
            if on {
                crate::permissions::ask_camera();
            }
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

// ── Shortcut recorder ──────────────────────────────────────────────────────

fn set_key_title(id: u16, text: &str) {
    if let Some(b) = control::<NSButton>(id) {
        b.setTitle(&ns(text));
    }
}

fn start_recording_key(id: u16) {
    stop_recording_key();
    hotkey::suspend();
    set_key_title(id, &tr("Press shortcut…"));
    let block = RcBlock::new(move |e: NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { e.as_ref() };
        let code = e.keyCode();
        let mods = (e.modifierFlags().0 as u32) & keys::MOD_MASK;
        if code == vk::ESCAPE {
            util::main_async(move || finish_recording_key(id, None));
            return std::ptr::null_mut();
        }
        let sc = Shortcut { key: code, mods };
        if sc.is_valid() {
            util::main_async(move || finish_recording_key(id, Some(sc)));
        } else {
            util::main_async(|| {
                STATE.with(|s| {
                    if let Some(s) = s.borrow().as_ref() {
                        s.hotkey_note.setStringValue(&ns(&tr("Add ⌘, ⌃ or ⌥ to the key. Esc cancels.")));
                    }
                })
            });
        }
        std::ptr::null_mut()
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
    if let Some(m) = monitor {
        STATE.with(|s| {
            if let Some(s) = s.borrow_mut().as_mut() {
                s.recording_key = Some((id, m));
            }
        });
    }
}

fn stop_recording_key() {
    let m = STATE.with(|s| s.borrow_mut().as_mut().and_then(|s| s.recording_key.take()));
    if let Some((_, m)) = m {
        unsafe { NSEvent::removeMonitor(&m) };
        hotkey::resume();
    }
}

fn finish_recording_key(id: u16, sc: Option<Shortcut>) {
    stop_recording_key();
    if let Some(sc) = sc {
        settings::update(|s| if id == SHOT_KEY { s.shot_hotkey = sc } else { s.record_hotkey = sc });
        let errors = app::apply_hotkeys();
        let note = if errors.is_empty() { tr("Saved.") } else { errors.join(" ") };
        STATE.with(|s| {
            if let Some(s) = s.borrow().as_ref() {
                s.hotkey_note.setStringValue(&ns(&note));
            }
        });
    }
    let s = settings::get();
    set_key_title(id, &if id == SHOT_KEY { s.shot_hotkey } else { s.record_hotkey }.label());
}

#[allow(unused)]
fn _keep(_: &NSString) {}
