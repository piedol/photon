//! A capture session: from the hotkey to copy/save/close. It owns the frozen
//! frames, the overlay windows and the markup, and is dropped completely when it
//! ends. Selection and markup logic are the Mac app's shared modules.

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_BITMAP_PROPERTIES, ID2D1Bitmap,
    ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::WindowsAndMessaging::{
    IDC_ARROW, IDC_CROSS, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW,
    SetCursor, SetForegroundWindow,
};
use windows::Win32::Foundation::COLORREF;

use crate::capture::{self, Display, Snapshot};
use crate::commands::Command;
use crate::d2d::{self, rgba};
use crate::geometry::{pt, Point, Rect};
use crate::i18n::{tr, trf};
use crate::keys::vk;
use crate::model::{Document, Draft, Rgba, Shape, Style, Tool, PALETTE};
use crate::overlay::{self, MouseKind, OverlayWindow};
use crate::render::{self, Source};
use crate::selection::{Handle, Release, Selection};
use crate::toolbar::{Btn, DrawState, Mode, Toolbars};
use crate::util;
use crate::{export, hotkey, hud, pin, record, settings};

pub struct Screen {
    pub display: Display,
    pub pixels: Vec<u8>,
    /// View-local bounds in physical pixels: (0, 0, w, h).
    pub bounds: Rect,
    win: OverlayWindow,
    bitmap: Option<ID2D1Bitmap>,
    /// The frozen frame pre-darkened once (the 45% dim), so no per-paint blending.
    dimmed: Option<ID2D1Bitmap>,
    pub sel: Selection,
    hover: Option<Rect>,
    hover_window: Option<(isize, Option<String>)>,
    snap_rects: Vec<(isize, Option<String>, Rect)>,
    picked_window: Option<(isize, Option<String>)>,
}

struct TextEdit {
    origin: Point,
    text: String,
    style: Style,
}

pub struct Session {
    pub mode: Mode,
    screens: Vec<Screen>,
    active: usize,
    toolbars: Toolbars,
    tool: Option<Tool>,
    color: Rgba,
    color_index: Option<usize>,
    stroke: usize,
    doc: Document,
    draft: Option<Draft>,
    text: Option<TextEdit>,
    front_app: Option<String>,
    last_point: Point,
    scroll_acc: f64,
    hover_btn: Option<Btn>,
    pressed_btn: Option<Btn>,
    mic: bool,
    audio: bool,
    cam: bool,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

pub fn is_open() -> bool {
    SESSION.with(|s| s.borrow().is_some())
}

pub fn mode() -> Option<Mode> {
    SESSION.with(|s| s.borrow().as_ref().map(|s| s.mode))
}

fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Begin a capture (screenshot or record-area selection).
pub fn start(mode: Mode) {
    if let Some(open) = self::mode() {
        if open == mode {
            close();
        }
        return;
    }
    let front_app = capture::frontmost_app_name();
    let t0 = std::time::Instant::now();
    match capture::capture_all() {
        Ok(snapshot) => {
            open(mode, snapshot, front_app);
            util::log(format!("overlay shown in {} ms", t0.elapsed().as_millis()));
        }
        Err(e) => hud::message(&tr("Couldn't capture the screen"), Some(&e)),
    }
}

fn open(mode: Mode, snapshot: Snapshot, front_app: Option<String>) {
    let s = settings::get();
    let cursor = cursor_pos();
    let mut screens = Vec::new();
    let mut active = 0;
    for (i, shot) in snapshot.shots.into_iter().enumerate() {
        let d = shot.display.clone();
        let bounds = Rect::new(0.0, 0.0, d.frame.w, d.frame.h);
        if d.frame.contains(cursor) {
            active = i;
        }
        let snap_rects = snapshot
            .windows
            .iter()
            .filter_map(|w| {
                let r = w.rect.translate(-d.frame.x, -d.frame.y).intersection(&bounds)?;
                Some((w.hwnd, w.app.clone(), r))
            })
            .collect();
        screens.push(Screen {
            win: OverlayWindow::new(d.frame, i),
            // Selections snap to whole pixels (coordinates are already pixels).
            sel: Selection::new(bounds, 1.0),
            display: d,
            pixels: shot.pixels,
            bounds,
            bitmap: None,
            dimmed: None,
            hover: None,
            hover_window: None,
            snap_rects,
            picked_window: None,
        });
    }
    if screens.is_empty() {
        return;
    }
    let scale = screens[active].display.scale;
    let session = Session {
        mode,
        screens,
        active,
        toolbars: Toolbars::new(mode, scale),
        tool: None,
        color: PALETTE[s.tool_color.min(PALETTE.len() - 1)],
        color_index: Some(s.tool_color),
        stroke: s.stroke,
        doc: Document::default(),
        draft: None,
        text: None,
        front_app,
        last_point: pt(0.0, 0.0),
        scroll_acc: 0.0,
        hover_btn: None,
        pressed_btn: None,
        mic: s.rec_mic,
        audio: s.rec_system_audio,
        cam: s.rec_webcam,
    };
    SESSION.with(|cell| *cell.borrow_mut() = Some(session));
    hotkey::set_escape(true);
    with(|s| {
        for sc in &s.screens {
            sc.win.show();
        }
        let a = &s.screens[s.active];
        unsafe {
            let _ = SetForegroundWindow(a.win.hwnd);
        }
        let local = pt(cursor.x - a.display.frame.x, cursor.y - a.display.frame.y);
        let idx = s.active;
        s.update_hover(idx, local);
        for sc in &s.screens {
            sc.win.invalidate();
        }
    });
}

fn cursor_pos() -> Point {
    let mut p = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
    }
    pt(p.x as f64, p.y as f64)
}

/// End the session and free everything.
pub fn close() {
    let taken = SESSION.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
    if let Some(s) = taken {
        drop(s);
        if !record::is_counting_down() {
            hotkey::set_escape(false);
        }
        util::release_memory_soon();
    }
}

impl Session {
    fn style(&self) -> Style {
        Style { color: self.color, size: self.stroke }
    }

    fn screen(&self) -> &Screen {
        &self.screens[self.active]
    }

    fn scale(&self) -> f64 {
        self.screen().display.scale
    }

    fn invalidate(&self, idx: usize) {
        self.screens[idx].win.invalidate();
    }

    fn relayout(&mut self) {
        let idx = self.active;
        let sc = &self.screens[idx];
        let px = sc.sel.rect.map(|r| (r.w.round() as u32, r.h.round() as u32)).unwrap_or((0, 0));
        let label = match (&sc.picked_window, self.mode) {
            (Some((_, Some(app))), Mode::Record) => Some(trf("{app} window only", &[("app", app.as_str())])),
            (Some((_, None)), Mode::Record) => Some(tr("This window only")),
            _ => None,
        };
        let (rect, bounds, dragging) = (sc.sel.rect, sc.bounds, sc.sel.is_dragging());
        self.toolbars.set_scale(sc.display.scale);
        self.toolbars.layout(rect, bounds, px, dragging, label.as_deref());
        self.invalidate(idx);
    }

    fn update_hover(&mut self, idx: usize, p: Point) {
        for (i, sc) in self.screens.iter_mut().enumerate() {
            let hit = (i == idx && sc.sel.rect.is_none()).then(|| sc.snap_rects.iter().find(|(_, _, r)| r.contains(p))).flatten();
            sc.hover_window = hit.map(|(id, app, _)| (*id, app.clone()));
            let new = if i == idx && sc.sel.rect.is_none() { Some(hit.map(|h| h.2).unwrap_or(sc.bounds)) } else { None };
            if sc.hover != new {
                sc.hover = new;
                sc.win.invalidate();
            }
        }
    }

    fn set_active(&mut self, idx: usize) {
        if idx == self.active {
            return;
        }
        let old = self.active;
        self.screens[old].sel.rect = None;
        self.screens[old].picked_window = None;
        self.active = idx;
        self.toolbars.clear();
        self.invalidate(old);
        self.relayout();
    }

    fn select_tool(&mut self, tool: Option<Tool>) {
        self.commit_text();
        self.tool = if self.tool == tool { None } else { tool };
        self.toolbars.swatches_shown = false;
        self.relayout();
    }

    fn set_stroke(&mut self, size: usize) {
        self.stroke = size.min(2);
        let s = self.stroke;
        settings::update(|st| st.stroke = s);
        if let Some(t) = &mut self.text {
            t.style.size = s;
        }
        self.invalidate(self.active);
    }

    fn set_color(&mut self, c: Rgba, index: Option<usize>) {
        self.color = c;
        self.color_index = index;
        if let Some(i) = index {
            settings::update(|st| st.tool_color = i);
        }
        if let Some(t) = &mut self.text {
            t.style.color = c;
        }
        self.invalidate(self.active);
    }

    fn begin_text(&mut self, p: Point) {
        self.commit_text();
        let style = self.style();
        let (_, h) = render::text_size("", style.font_size(), self.scale());
        self.text = Some(TextEdit { origin: pt(p.x, p.y - h / 2.0), text: String::new(), style });
        self.invalidate(self.active);
    }

    fn commit_text(&mut self) {
        let Some(t) = self.text.take() else { return };
        if !t.text.trim().is_empty() {
            self.doc.push(Shape::Text { origin: t.origin, text: t.text, style: t.style });
        }
        self.invalidate(self.active);
    }

    fn cancel_text(&mut self) {
        if self.text.take().is_some() {
            self.invalidate(self.active);
        }
    }

    fn mouse(&mut self, idx: usize, kind: MouseKind, p: Point, shift: bool) -> Option<Command> {
        self.last_point = p;
        let on_bar = idx == self.active && self.toolbars.contains(p);

        // Toolbar buttons.
        if idx == self.active {
            let hit = self.toolbars.hit(p);
            match kind {
                MouseKind::Move | MouseKind::Drag if self.pressed_btn.is_none() || kind == MouseKind::Move => {
                    if hit != self.hover_btn {
                        self.hover_btn = hit;
                        self.invalidate(idx);
                    }
                }
                MouseKind::Down if on_bar => {
                    self.pressed_btn = hit;
                    return None;
                }
                MouseKind::Up if self.pressed_btn.is_some() => {
                    let pressed = self.pressed_btn.take();
                    if pressed.is_some() && pressed == hit {
                        return pressed.map(btn_command);
                    }
                    return None;
                }
                _ => {}
            }
            if on_bar && kind != MouseKind::Drag {
                return None;
            }
        }

        if kind == MouseKind::Down {
            if self.text.is_some() {
                self.commit_text();
                return None;
            }
            if self.toolbars.swatches_shown {
                self.toolbars.swatches_shown = false;
                self.relayout();
            }
            if idx != self.active {
                if !self.doc.is_empty() {
                    return None;
                }
                self.set_active(idx);
            }
        } else if idx != self.active {
            if kind == MouseKind::Move {
                self.update_hover(idx, p);
            }
            return None;
        }

        match kind {
            MouseKind::Down => {
                let allow_new = self.doc.is_empty();
                let allow_move = self.tool.is_none();
                let sc = &mut self.screens[idx];
                if sc.sel.press(p, allow_new, allow_move) {
                    self.relayout();
                } else if let (Some(tool), Some(r)) = (self.tool, self.screens[idx].sel.rect)
                    && r.contains(p) {
                        if tool == Tool::Text {
                            self.begin_text(p);
                        } else {
                            self.draft = Some(Draft::begin(tool, self.style(), p));
                            self.invalidate(idx);
                        }
                    }
            }
            MouseKind::Drag => {
                if let Some(d) = &mut self.draft {
                    let r = self.screens[idx].sel.rect.unwrap_or(self.screens[idx].bounds);
                    d.update(pt(p.x.clamp(r.x, r.max_x()), p.y.clamp(r.y, r.max_y())), shift);
                    self.invalidate(idx);
                } else if self.screens[idx].sel.is_dragging() {
                    self.screens[idx].sel.drag_to(p);
                    self.screens[idx].hover = None;
                    self.relayout();
                }
            }
            MouseKind::Up => {
                if let Some(d) = self.draft.take() {
                    if let Some(shape) = d.finish(self.doc.next_step()) {
                        self.doc.push(shape);
                    }
                    self.invalidate(idx);
                } else {
                    let sc = &mut self.screens[idx];
                    match sc.sel.release(p) {
                        Release::Click(_) => {
                            let target = sc.hover.unwrap_or(sc.bounds);
                            sc.sel.set(target);
                            sc.picked_window = sc.hover_window.take();
                        }
                        Release::Changed => sc.picked_window = None,
                        Release::Nothing => {}
                    }
                    sc.hover = None;
                    self.relayout();
                }
            }
            MouseKind::Move => {
                if !self.screens[idx].sel.is_dragging() {
                    self.update_hover(idx, p);
                }
            }
            MouseKind::DoubleClick => {}
        }
        None
    }

    fn key(&mut self, code: u16) -> Option<Command> {
        let (shift, ctrl, alt) = overlay::modifiers();
        if self.text.is_some() {
            return match code {
                vk::RETURN if !shift => {
                    self.commit_text();
                    Some(Command::DismissHud)
                }
                vk::ESCAPE => {
                    self.cancel_text();
                    Some(Command::DismissHud)
                }
                _ => None, // characters arrive via WM_CHAR
            };
        }
        if ctrl {
            return match code {
                vk::C => Some(Command::Copy),
                vk::S => Some(Command::Save),
                vk::P => Some(Command::Print),
                vk::Z => Some(Command::Undo),
                _ => None,
            };
        }
        match code {
            vk::ESCAPE => return Some(Command::Escape),
            vk::RETURN => {
                self.screen().sel.rect?;
                return Some(if self.mode == Mode::Record { Command::StartRecording } else { Command::Copy });
            }
            vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN => {
                let step = if shift { 10.0 } else { 1.0 };
                let (dx, dy) = match code {
                    vk::LEFT => (-step, 0.0),
                    vk::RIGHT => (step, 0.0),
                    vk::UP => (0.0, -step),
                    _ => (0.0, step),
                };
                let a = self.active;
                self.screens[a].picked_window = None;
                if self.screens[a].sel.nudge(dx, dy, alt) {
                    self.relayout();
                }
            }
            vk::OEM_4 => self.set_stroke(self.stroke.saturating_sub(1)),
            vk::OEM_6 => self.set_stroke(self.stroke + 1),
            _ => return None,
        }
        Some(Command::DismissHud)
    }

    fn paint(&mut self, idx: usize, hdc: windows::Win32::Graphics::Gdi::HDC) {
        let scale = self.screens[idx].display.scale;
        let Some(rt) = self.screens[idx].win.target(hdc) else { return };
        let sc = &mut self.screens[idx];
        if sc.bitmap.is_none() {
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let (w, h) = (sc.bounds.w as u32, sc.bounds.h as u32);
            sc.bitmap = unsafe { rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(sc.pixels.as_ptr() as _), w * 4, &props) }.ok();
            let dark: Vec<u8> = sc.pixels.iter().map(|&v| ((v as u32 * 141) >> 8) as u8).collect();
            sc.dimmed = unsafe { rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(dark.as_ptr() as _), w * 4, &props) }.ok();
        }
        let sc = &self.screens[idx];
        let b = sc.bounds;
        unsafe {
            rt.BeginDraw();
            // Dimmed frame everywhere, then the selection (or the window under
            // the pointer) at full brightness.
            if let Some(dark) = &sc.dimmed {
                rt.DrawBitmap(dark, Some(&d2d::rect(0.0, 0.0, b.w, b.h)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None);
            }
            if let (Some(bmp), Some(h)) = (&sc.bitmap, sc.sel.rect.or(sc.hover)) {
                let r = d2d::rect(h.x, h.y, h.w, h.h);
                rt.DrawBitmap(bmp, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, Some(&r));
            }
            if let (None, Some(h)) = (sc.sel.rect, sc.hover) {
                let a = d2d::brush(&rt, overlay::accent(0.10));
                rt.FillRectangle(&d2d::rect(h.x, h.y, h.w, h.h), &a);
                let a = d2d::brush(&rt, overlay::accent(0.95));
                rt.DrawRectangle(&d2d::rect(h.x + 1.0, h.y + 1.0, h.w - 2.0, h.h - 2.0), &a, (2.0 * scale) as f32, None);
            }
            if let Some(r) = sc.sel.rect {
                if idx == self.active {
                    rt.PushAxisAlignedClip(&d2d::rect(r.x, r.y, r.w, r.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    let src = Source { pixels: &sc.pixels, width: b.w as u32, height: b.h as u32, scale };
                    render::draw_shapes(&rt, &self.doc.shapes, &src);
                    if let Some(shape) = self.draft.as_ref().and_then(|d| d.shape(self.doc.next_step())) {
                        render::draw_shape(&rt, &shape, &src);
                    }
                    if let Some(t) = &self.text {
                        draw_text_edit(&rt, t, scale);
                    }
                    rt.PopAxisAlignedClip();
                }
                // Border with a soft dark halo so it reads on any background.
                let halo = d2d::brush(&rt, rgba(0.0, 0.0, 0.0, 0.35));
                rt.DrawRectangle(&d2d::rect(r.x - 1.5, r.y - 1.5, r.w + 3.0, r.h + 3.0), &halo, 1.0, None);
                let white = d2d::brush(&rt, rgba(1.0, 1.0, 1.0, 0.95));
                rt.DrawRectangle(&d2d::rect(r.x - 0.5, r.y - 0.5, r.w + 1.0, r.h + 1.0), &white, 1.0, None);
                if idx == self.active && !sc.sel.is_dragging() {
                    let hs = 7.0 * scale;
                    let edge = d2d::brush(&rt, rgba(0.0, 0.0, 0.0, 0.35));
                    for h in Handle::ALL {
                        let c = h.center(&r);
                        let hr = d2d::rect(c.x - hs / 2.0, c.y - hs / 2.0, hs, hs);
                        rt.FillRectangle(&hr, &white);
                        rt.DrawRectangle(&hr, &edge, 1.0, None);
                    }
                }
            }
            if idx == self.active {
                let st = DrawState {
                    tool: self.tool,
                    color: self.color,
                    color_index: self.color_index,
                    stroke: self.stroke,
                    mic: self.mic,
                    audio: self.audio,
                    cam: self.cam,
                    hover: self.hover_btn,
                };
                self.toolbars.draw(&rt, &st);
                if let Some(btn) = self.hover_btn {
                    draw_tooltip(&rt, &btn.tooltip(), self.last_point, b, scale);
                }
            }
            let _ = rt.EndDraw(None, None);
        }
        sc.win.validate();
    }

    fn cursor(&self, idx: usize) -> windows::core::PCWSTR {
        let sc = &self.screens[idx];
        let p = self.last_point;
        if idx == self.active && self.toolbars.contains(p) {
            return IDC_ARROW;
        }
        if idx == self.active {
            if let Some(h) = sc.sel.handle_at(p) {
                return match h {
                    Handle::N | Handle::S => IDC_SIZENS,
                    Handle::E | Handle::W => IDC_SIZEWE,
                    Handle::NW | Handle::SE => IDC_SIZENESW,
                    Handle::NE | Handle::SW => IDC_SIZENWSE,
                };
            }
            if let Some(r) = sc.sel.rect
                && r.contains(p) {
                    return match self.tool {
                        None => IDC_SIZEALL,
                        Some(Tool::Text) => IDC_IBEAM,
                        Some(_) => IDC_CROSS,
                    };
                }
        }
        IDC_CROSS
    }
}

/// Handle names are AppKit (y-up); on screen (y-down) N/S swap, so the
/// diagonal cursors above are crossed on purpose.
fn btn_command(b: Btn) -> Command {
    match b {
        Btn::Tool(t) => Command::Tool(t.index()),
        Btn::Color => Command::ToggleSwatches,
        Btn::Stroke => Command::CycleStroke,
        Btn::Undo => Command::Undo,
        Btn::Swatch(i) => Command::Color(i as u8),
        Btn::Custom => Command::CustomColor,
        Btn::Record => Command::RecordSelection,
        Btn::Pin => Command::Pin,
        Btn::Print => Command::Print,
        Btn::Save => Command::Save,
        Btn::Copy => Command::Copy,
        Btn::Close => Command::Close,
        Btn::Mic => Command::ToggleMic,
        Btn::Audio => Command::ToggleSystemAudio,
        Btn::Cam => Command::ToggleWebcam,
        Btn::Start => Command::StartRecording,
    }
}

fn draw_text_edit(rt: &ID2D1RenderTarget, t: &TextEdit, k: f64) {
    let shape = Shape::Text { origin: t.origin, text: t.text.clone(), style: t.style };
    let src = Source { pixels: &[], width: 0, height: 0, scale: k };
    render::draw_shape(rt, &shape, &src);
    // Caret at the end of the text.
    let l = d2d::layout(if t.text.is_empty() { " " } else { &t.text }, t.style.font_size() * k, d2d::WEIGHT_SEMIBOLD);
    let (mut x, mut y) = (0f32, 0f32);
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    let pos = if t.text.is_empty() { 0 } else { t.text.encode_utf16().count() as u32 };
    unsafe {
        let _ = l.HitTestTextPosition(pos, false, &mut x, &mut y, &mut m);
        let c = t.style.color;
        let b = d2d::brush(rt, rgba(c.0 as f32, c.1 as f32, c.2 as f32, 1.0));
        let cx = t.origin.x + x as f64 + 1.0;
        rt.DrawLine(d2d::v2(cx, t.origin.y + y as f64 + 2.0), d2d::v2(cx, t.origin.y + y as f64 + m.height as f64 - 2.0), &b, (1.5 * k) as f32, None);
    }
}

fn draw_tooltip(rt: &ID2D1RenderTarget, text: &str, p: Point, bounds: Rect, s: f64) {
    let l = d2d::layout(text, 12.0 * s, d2d::WEIGHT_REGULAR);
    let (w, h) = d2d::measure(&l);
    let r = Rect::new(p.x + 14.0 * s, p.y + 22.0 * s, w + 14.0 * s, h + 8.0 * s).clamp_inside(&bounds.inset(4.0));
    unsafe {
        rt.FillRoundedRectangle(&d2d::rr(r.x, r.y, r.w, r.h, 5.0 * s), &d2d::brush(rt, rgba(0.08, 0.08, 0.09, 0.96)));
        rt.DrawTextLayout(d2d::v2(r.x + 7.0 * s, r.y + 4.0 * s), &l, &d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.92)), windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

// ── Entry points from the overlay window ───────────────────────────────────

pub fn paint(idx: usize, hdc: windows::Win32::Graphics::Gdi::HDC) {
    let t0 = std::time::Instant::now();
    with(|s| {
        if idx < s.screens.len() {
            s.paint(idx, hdc)
        }
    });
    if util::log_enabled() {
        util::log(format!("paint {idx} took {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0));
    }
}

pub fn mouse(idx: usize, kind: MouseKind, p: Point) {
    let (shift, _, _) = overlay::modifiers();
    if let Some(Some(cmd)) = with(|s| if idx < s.screens.len() { s.mouse(idx, kind, p, shift) } else { None }) {
        command(cmd);
    }
}

pub fn right_click() {
    close();
}

pub fn set_cursor(idx: usize) {
    let c = with(|s| if idx < s.screens.len() { s.cursor(idx) } else { IDC_CROSS }).unwrap_or(IDC_CROSS);
    unsafe {
        if let Ok(h) = LoadCursorW(None, c) {
            SetCursor(Some(h));
        }
    }
}

pub fn scroll(delta: f64) {
    with(|s| {
        s.scroll_acc += delta;
        if s.scroll_acc.abs() >= 120.0 {
            let up = s.scroll_acc > 0.0;
            s.scroll_acc = 0.0;
            let next = if up { s.stroke + 1 } else { s.stroke.saturating_sub(1) };
            s.set_stroke(next);
        }
    });
}

pub fn key_down(code: u16) -> bool {
    match with(|s| s.key(code)) {
        Some(Some(Command::DismissHud)) => true,
        Some(Some(cmd)) => {
            command(cmd);
            true
        }
        _ => false,
    }
}

pub fn char_input(c: u32) -> bool {
    let (shift, ctrl, _) = overlay::modifiers();
    with(|s| {
        let Some(t) = &mut s.text else { return false };
        match c {
            0x08 => {
                t.text.pop();
            }
            0x0D if shift => t.text.push('\n'),
            0x0D | 0x1B => return true,
            c if c < 0x20 || ctrl => return true,
            c => {
                if let Some(ch) = char::from_u32(c) {
                    t.text.push(ch);
                }
            }
        }
        s.invalidate(s.active);
        true
    })
    .unwrap_or(false)
}

/// Esc: cancel text, then the swatch strip, then the whole selection.
pub fn escape() {
    let close_all = with(|s| {
        if s.text.is_some() {
            s.cancel_text();
            false
        } else if s.toolbars.swatches_shown {
            s.toolbars.swatches_shown = false;
            s.relayout();
            false
        } else {
            true
        }
    })
    .unwrap_or(false);
    if close_all {
        close();
    }
}

/// The finished screenshot.
pub struct Finished {
    pub image: export::Image,
    pub display_name: String,
    pub front_app: Option<String>,
    /// Selection in virtual-screen pixels (for pinning in place).
    pub global_rect: Rect,
    pub scale: f64,
}

fn finish_image() -> Option<Finished> {
    with(|s| {
        s.commit_text();
        let sc = s.screen();
        let r = sc.sel.rect?;
        let image = export::compose(&sc.pixels, sc.bounds.w as u32, sc.bounds.h as u32, r, &s.doc.shapes, sc.display.scale)?;
        Some(Finished {
            image,
            display_name: sc.display.name.clone(),
            front_app: s.front_app.clone(),
            global_rect: r.translate(sc.display.frame.x, sc.display.frame.y),
            scale: sc.display.scale,
        })
    })
    .flatten()
}

/// What to record, from the active selection.
#[derive(Clone, Debug)]
pub struct RecordTarget {
    pub monitor: isize,
    /// Region in monitor-local pixels.
    pub region: Rect,
    /// Region in virtual-screen pixels.
    pub global_rect: Rect,
    pub scale: f64,
    pub display_name: String,
    pub front_app: Option<String>,
    /// Record only this window (follows it, ignores everything around it).
    pub window: Option<isize>,
}

fn record_target() -> Option<RecordTarget> {
    with(|s| {
        let sc = s.screen();
        let r = sc.sel.rect?;
        Some(RecordTarget {
            monitor: sc.display.handle,
            region: r,
            global_rect: r.translate(sc.display.frame.x, sc.display.frame.y),
            scale: sc.display.scale,
            display_name: sc.display.name.clone(),
            front_app: sc.picked_window.as_ref().and_then(|w| w.1.clone()).or_else(|| s.front_app.clone()),
            window: sc.picked_window.as_ref().map(|w| w.0),
        })
    })
    .flatten()
}

fn active_hwnd() -> Option<HWND> {
    with(|s| s.screen().win.hwnd)
}

/// Overlay commands from toolbars and keys.
pub fn command(cmd: Command) {
    match cmd {
        Command::Close => close(),
        Command::Escape => escape(),
        Command::Tool(i) => {
            with(|s| s.select_tool(Tool::from_index(i)));
        }
        Command::Color(i) => {
            with(|s| {
                if let Some(c) = PALETTE.get(i as usize) {
                    s.set_color(*c, Some(i as usize));
                }
                s.toolbars.swatches_shown = false;
                s.relayout();
            });
        }
        Command::ToggleSwatches => {
            with(|s| {
                s.toolbars.swatches_shown = !s.toolbars.swatches_shown;
                s.relayout();
            });
        }
        Command::CustomColor => {
            let owner = active_hwnd();
            let start = with(|s| s.color).unwrap_or(PALETTE[0]);
            with(|s| {
                s.toolbars.swatches_shown = false;
                s.relayout();
            });
            if let Some(c) = choose_color(owner, start) {
                with(|s| s.set_color(c, None));
            }
        }
        Command::CycleStroke => {
            with(|s| s.set_stroke((s.stroke + 1) % 3));
        }
        Command::Stroke(i) => {
            with(|s| s.set_stroke(i as usize));
        }
        Command::Undo => {
            with(|s| {
                if s.text.is_some() {
                    s.cancel_text();
                } else if s.doc.undo().is_some() {
                    s.invalidate(s.active);
                }
            });
        }
        Command::Copy => {
            if let Some(f) = finish_image() {
                close();
                export::copy(&f);
            }
        }
        Command::Save => {
            if let Some(f) = finish_image() {
                close();
                export::save_with_dialog(f);
            }
        }
        Command::Print => {
            if let Some(f) = finish_image() {
                close();
                export::print(&f);
            }
        }
        Command::Pin => {
            if let Some(f) = finish_image() {
                close();
                pin::show(f);
            }
        }
        Command::RecordSelection | Command::StartRecording => {
            if let Some(t) = record_target() {
                close();
                record::start(t);
            }
        }
        Command::ToggleMic | Command::ToggleSystemAudio | Command::ToggleWebcam => {
            let st = settings::update(|st| match cmd {
                Command::ToggleMic => st.rec_mic = !st.rec_mic,
                Command::ToggleSystemAudio => st.rec_system_audio = !st.rec_system_audio,
                _ => st.rec_webcam = !st.rec_webcam,
            });
            with(|s| {
                s.mic = st.rec_mic;
                s.audio = st.rec_system_audio;
                s.cam = st.rec_webcam;
                s.invalidate(s.active);
            });
        }
        _ => {}
    }
}

/// The Windows colour picker, above the overlay.
fn choose_color(owner: Option<HWND>, start: Rgba) -> Option<Rgba> {
    static mut CUSTOM: [COLORREF; 16] = [COLORREF(0x00FFFFFF); 16];
    let to_ref = |c: Rgba| COLORREF(((c.2 * 255.0) as u32) << 16 | ((c.1 * 255.0) as u32) << 8 | (c.0 * 255.0) as u32);
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        rgbResult: to_ref(start),
        // SAFETY: only touched on the UI thread while the modal dialog runs.
        lpCustColors: std::ptr::addr_of_mut!(CUSTOM) as *mut COLORREF,
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    if unsafe { ChooseColorW(&mut cc) }.as_bool() {
        let v = cc.rgbResult.0;
        Some(Rgba((v & 0xff) as f64 / 255.0, ((v >> 8) & 0xff) as f64 / 255.0, ((v >> 16) & 0xff) as f64 / 255.0, 1.0))
    } else {
        None
    }
}


