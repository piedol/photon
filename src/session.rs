//! A capture session: from the hotkey to copy/save/close. It owns the frozen
//! frames, the overlay windows and the markup, and is dropped completely when it
//! ends, so nothing large stays in memory between captures.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSColor, NSColorPanel, NSCursor, NSEvent, NSEventModifierFlags, NSFocusRingType, NSTextField, NSView,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGContext, CGImage};
use objc2_foundation::NSRect;

use crate::annotate::model::{Document, Draft, Rgba, Shape, Style, Tool, PALETTE};
use crate::annotate::render::{self, Source};
use crate::capture::{self, cg_rect, Display, Snapshot};
use crate::commands::Command;
use crate::geometry::{pt, Point, Rect};
use crate::keys::vk;
use crate::overlay::chrome::Chrome;
use crate::overlay::selection::{Release, Selection};
use crate::overlay::toolbar::{Mode, Toolbars};
use crate::overlay::window::{make_window, AnnotView, OverlayView, OverlayWindow};
use crate::util::{self, ns};
use crate::{export, permissions, pin, record, settings, ui};

pub use crate::overlay::toolbar::Mode as SessionMode;

pub struct Screen {
    pub display: Display,
    pub image: CFRetained<CGImage>,
    /// View-local bounds: (0, 0, width, height) in points.
    pub bounds: Rect,
    window: Retained<OverlayWindow>,
    view: Retained<OverlayView>,
    annot: Retained<AnnotView>,
    chrome: Chrome,
    pub sel: Selection,
    hover: Option<Rect>,
    snap_rects: Vec<Rect>,
}

struct TextEdit {
    field: Retained<NSTextField>,
    style: Style,
}

pub struct Session {
    pub mode: Mode,
    pub screens: Vec<Screen>,
    pub active: usize,
    toolbars: Toolbars,
    pub tool: Option<Tool>,
    color: Rgba,
    color_index: Option<usize>,
    stroke: usize,
    pub doc: Document,
    draft: Option<Draft>,
    text: Option<TextEdit>,
    pub front_app: Option<String>,
    last_point: Point,
    scroll_acc: f64,
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

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().unwrap()
}

/// Begin a capture (screenshot or record-area selection).
pub fn start(mode: Mode) {
    if let Some(open) = self::mode() {
        // Pressing the hotkey again while the overlay is up cancels it.
        if open == mode {
            close();
        }
        return;
    }
    if !permissions::screen_ok() {
        permissions::ask_screen();
        return;
    }
    let front_app = capture::frontmost_app_name();
    capture::capture_all(mtm(), move |result| match result {
        Ok(snapshot) => open(mode, snapshot, front_app),
        Err(e) => {
            util::log(format!("capture failed: {e}"));
            crate::hud::message("Couldn't capture the screen", Some(&e));
        }
    });
}

fn open(mode: Mode, snapshot: Snapshot, front_app: Option<String>) {
    let mtm = mtm();
    let s = settings::get();
    let mouse = NSEvent::mouseLocation();
    let mut screens = Vec::new();
    let mut active = 0;
    for (i, shot) in snapshot.shots.into_iter().enumerate() {
        let d = &shot.display;
        let bounds = Rect::new(0.0, 0.0, d.frame.w, d.frame.h);
        if d.frame.contains(pt(mouse.x, mouse.y)) {
            active = i;
        }
        let window = make_window(mtm, cg_rect(d.frame));
        let view = OverlayView::new(mtm, cg_rect(bounds), i);
        view.setWantsLayer(true);

        let image_view = NSView::initWithFrame(NSView::alloc(mtm), cg_rect(bounds));
        image_view.setWantsLayer(true);
        if let Some(layer) = image_view.layer() {
            // SAFETY: CGImage is a CF type and valid layer contents.
            let obj: &AnyObject = unsafe { &*(CFRetained::as_ptr(&shot.image).as_ptr() as *const AnyObject) };
            unsafe { layer.setContents(Some(obj)) };
            layer.setContentsScale(d.scale);
        }
        let chrome = Chrome::new(mtm, bounds, d.scale);
        let annot = AnnotView::new(mtm, cg_rect(bounds), i);
        annot.setWantsLayer(true);

        view.addSubview(&image_view);
        view.addSubview(&chrome.view);
        view.addSubview(&annot);
        window.setContentView(Some(&view));

        let snap_rects = snapshot
            .windows
            .iter()
            .filter_map(|w| w.translate(-d.frame.x, -d.frame.y).intersection(&bounds))
            .collect();
        screens.push(Screen {
            sel: Selection::new(bounds, d.scale),
            display: shot.display,
            image: shot.image,
            bounds,
            window,
            view,
            annot,
            chrome,
            hover: None,
            snap_rects,
        });
    }
    if screens.is_empty() {
        return;
    }

    let toolbars = Toolbars::new(mtm, mode);
    let color = PALETTE[s.tool_color.min(PALETTE.len() - 1)];
    toolbars.set_color(color, Some(s.tool_color));
    toolbars.set_stroke(s.stroke);
    toolbars.set_tool(None);
    toolbars.set_record_toggles(s.rec_mic, s.rec_system_audio, s.rec_webcam);
    toolbars.attach(&screens[active].view);

    let session = Session {
        mode,
        screens,
        active,
        toolbars,
        tool: None,
        color,
        color_index: Some(s.tool_color),
        stroke: s.stroke,
        doc: Document::default(),
        draft: None,
        text: None,
        front_app,
        last_point: pt(0.0, 0.0),
        scroll_acc: 0.0,
    };
    SESSION.with(|cell| *cell.borrow_mut() = Some(session));

    let app = ui::app(mtm);
    app.activate();
    with(|s| {
        for (i, sc) in s.screens.iter().enumerate() {
            if i != s.active {
                sc.window.orderFront(None);
            }
        }
        let a = &s.screens[s.active];
        a.window.makeKeyAndOrderFront(None);
        a.window.makeFirstResponder(Some(&a.view));
        let local = pt(mouse.x - a.display.frame.x, mouse.y - a.display.frame.y);
        s.update_hover(s.active, local);
        NSCursor::crosshairCursor().set();
    });
}

/// End the session and free everything.
pub fn close() {
    let taken = SESSION.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
    if let Some(s) = taken {
        if NSColorPanel::sharedColorPanelExists(mtm()) {
            NSColorPanel::sharedColorPanel(mtm()).orderOut(None);
        }
        for sc in &s.screens {
            sc.window.orderOut(None);
            sc.window.close();
        }
        drop(s);
        ui::app(mtm()).deactivate();
        NSCursor::arrowCursor().set();
        util::release_memory_soon();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    Drag,
    Up,
    Move,
}

fn shift_held(e: &NSEvent) -> bool {
    e.modifierFlags().contains(NSEventModifierFlags::Shift)
}

impl Session {
    fn style(&self) -> Style {
        Style { color: self.color, size: self.stroke }
    }

    fn screen(&self) -> &Screen {
        &self.screens[self.active]
    }

    fn sel_rect(&self) -> Option<Rect> {
        self.screen().sel.rect
    }

    fn refresh(&self, idx: usize) {
        let sc = &self.screens[idx];
        let dragging = sc.sel.is_dragging();
        let handles = idx == self.active && !dragging;
        sc.chrome.update(sc.bounds, sc.sel.rect, sc.hover, handles, false);
        if idx == self.active {
            let px = sc.sel.rect.map(|r| r.pixel_size(sc.display.scale)).unwrap_or((0, 0));
            self.toolbars.layout(sc.sel.rect, sc.bounds, px, dragging);
        }
    }

    fn redraw_annotations(&self) {
        self.screen().annot.setNeedsDisplay(true);
    }

    fn update_hover(&mut self, idx: usize, p: Point) {
        for (i, sc) in self.screens.iter_mut().enumerate() {
            let new = if i == idx && sc.sel.rect.is_none() {
                Some(sc.snap_rects.iter().copied().find(|r| r.contains(p)).unwrap_or(sc.bounds))
            } else {
                None
            };
            if sc.hover != new {
                sc.hover = new;
                sc.chrome.update(sc.bounds, sc.sel.rect, sc.hover, false, false);
            }
        }
    }

    fn set_active(&mut self, idx: usize) {
        if idx == self.active {
            return;
        }
        let old = self.active;
        self.screens[old].sel.rect = None;
        self.active = idx;
        self.toolbars.attach(&self.screens[idx].view);
        self.refresh(old);
        self.refresh(idx);
        self.screens[old].annot.setNeedsDisplay(true);
    }

    fn over_toolbar(&self, p: Point) -> bool {
        self.toolbars.views().iter().any(|v| {
            !v.isHidden() && {
                let f = v.frame();
                Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height).contains(p)
            }
        })
    }

    fn cursor_for(&self, idx: usize, p: Point) {
        let sc = &self.screens[idx];
        let cursor = if self.over_toolbar(p) {
            NSCursor::arrowCursor()
        } else if sc.sel.is_dragging() && self.draft.is_none() && sc.sel.handle_at(p).is_none() && sc.sel.rect.is_some_and(|r| r.contains(p)) {
            NSCursor::closedHandCursor()
        } else if idx == self.active && self.tool.is_none() && sc.sel.handle_at(p).is_none() && sc.sel.rect.is_some_and(|r| r.contains(p)) {
            NSCursor::openHandCursor()
        } else if self.tool == Some(Tool::Text) && sc.sel.rect.is_some_and(|r| r.contains(p)) {
            NSCursor::IBeamCursor()
        } else {
            NSCursor::crosshairCursor()
        };
        cursor.set();
    }

    fn select_tool(&mut self, tool: Option<Tool>) {
        self.commit_text();
        self.tool = if self.tool == tool { None } else { tool };
        self.toolbars.set_tool(self.tool);
        if self.toolbars.swatches_shown() {
            self.toolbars.toggle_swatches();
        }
    }

    fn set_stroke(&mut self, size: usize) {
        self.stroke = size.min(2);
        self.toolbars.set_stroke(self.stroke);
        let s = self.stroke;
        settings::update(|st| st.stroke = s);
        if let Some(t) = &self.text {
            let font = render::text_font(Style { color: self.color, size: s }.font_size());
            t.field.setFont(Some(&font));
            resize_text_field(&t.field);
        }
    }

    fn set_color(&mut self, col: Rgba, index: Option<usize>) {
        self.color = col;
        self.color_index = index;
        self.toolbars.set_color(col, index);
        if let Some(i) = index {
            settings::update(|st| st.tool_color = i);
        }
        if let Some(t) = &self.text {
            t.field.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(col.0, col.1, col.2, 1.0)));
        }
    }

    fn begin_text(&mut self, p: Point) {
        self.commit_text();
        let mtm = mtm();
        let style = self.style();
        let font = render::text_font(style.font_size());
        let (_, h) = render::text_size("", style.font_size());
        let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), cg_rect(Rect::new(p.x - 2.0, p.y - h, 40.0, h + 2.0)));
        field.setEditable(true);
        field.setBezeled(false);
        field.setBordered(false);
        field.setDrawsBackground(false);
        field.setFocusRingType(NSFocusRingType::None);
        field.setFont(Some(&font));
        let c = style.color;
        field.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(c.0, c.1, c.2, 1.0)));
        if let Some(cell) = field.cell() {
            cell.setWraps(false);
            cell.setScrollable(true);
        }
        let delegate = ui::text_delegate(mtm);
        unsafe { field.setDelegate(Some(&delegate)) };
        let sc = self.screen();
        sc.view.addSubview(&field);
        sc.window.makeFirstResponder(Some(&field));
        self.text = Some(TextEdit { field, style });
    }

    /// Turn the inline editor into a text shape.
    fn commit_text(&mut self) {
        let Some(t) = self.text.take() else { return };
        let text = t.field.stringValue().to_string();
        let f = t.field.frame();
        t.field.removeFromSuperview();
        let sc = self.screen();
        sc.window.makeFirstResponder(Some(&sc.view));
        if !text.trim().is_empty() {
            let origin = pt(f.origin.x + 2.0, f.origin.y + f.size.height - 1.0);
            self.doc.push(Shape::Text { origin, text, style: Style { color: self.color, size: t.style.size } });
            self.redraw_annotations();
        }
    }

    fn cancel_text(&mut self) {
        if let Some(t) = self.text.take() {
            t.field.removeFromSuperview();
            let sc = self.screen();
            sc.window.makeFirstResponder(Some(&sc.view));
        }
    }

    fn mouse(&mut self, idx: usize, kind: MouseKind, p: Point, shift: bool) -> Option<Command> {
        self.last_point = p;
        if kind == MouseKind::Down {
            if self.text.is_some() {
                self.commit_text();
                return None;
            }
            if self.toolbars.swatches_shown() {
                self.toolbars.toggle_swatches();
            }
            if idx != self.active {
                if !self.doc.is_empty() {
                    return None; // keep markup on its display
                }
                self.set_active(idx);
            }
        } else if idx != self.active {
            if kind == MouseKind::Move {
                self.update_hover(idx, p);
                self.cursor_for(idx, p);
            }
            return None;
        }

        match kind {
            MouseKind::Down => {
                let allow_new = self.doc.is_empty();
                let allow_move = self.tool.is_none();
                let sc = &mut self.screens[idx];
                if sc.sel.press(p, allow_new, allow_move) {
                    self.refresh(idx);
                } else if let (Some(tool), Some(r)) = (self.tool, sc.sel.rect) {
                    if r.contains(p) {
                        if tool == Tool::Text {
                            self.begin_text(p);
                        } else {
                            self.draft = Some(Draft::begin(tool, self.style(), p));
                            self.redraw_annotations();
                        }
                    }
                }
            }
            MouseKind::Drag => {
                if let Some(d) = &mut self.draft {
                    let r = self.screens[idx].sel.rect.unwrap_or(self.screens[idx].bounds);
                    let q = pt(p.x.clamp(r.x, r.max_x()), p.y.clamp(r.y, r.max_y()));
                    d.update(q, shift);
                    self.redraw_annotations();
                } else if self.screens[idx].sel.is_dragging() {
                    self.screens[idx].sel.drag_to(p);
                    self.screens[idx].hover = None;
                    self.refresh(idx);
                    self.cursor_for(idx, p);
                }
            }
            MouseKind::Up => {
                if let Some(d) = self.draft.take() {
                    if let Some(shape) = d.finish(self.doc.next_step()) {
                        self.doc.push(shape);
                    }
                    self.redraw_annotations();
                } else {
                    let sc = &mut self.screens[idx];
                    if let Release::Click(_) = sc.sel.release(p) {
                        // Click without a drag: take the hovered window or the display.
                        let target = sc.hover.unwrap_or(sc.bounds);
                        sc.sel.set(target);
                    }
                    sc.hover = None;
                    self.refresh(idx);
                    self.cursor_for(idx, p);
                }
            }
            MouseKind::Move => {
                if !self.screens[idx].sel.is_dragging() {
                    self.update_hover(idx, p);
                }
                self.cursor_for(idx, p);
            }
        }
        None
    }

    fn key(&mut self, e: &NSEvent) -> Option<Command> {
        let code = e.keyCode();
        let flags = e.modifierFlags();
        let shift = flags.contains(NSEventModifierFlags::Shift);
        let option = flags.contains(NSEventModifierFlags::Option);
        match code {
            vk::ESCAPE => {
                if self.toolbars.swatches_shown() {
                    self.toolbars.toggle_swatches();
                    return Some(Command::DismissHud); // handled
                }
                return Some(Command::Close);
            }
            vk::RETURN | vk::KEYPAD_ENTER => {
                self.sel_rect()?;
                return Some(if self.mode == Mode::Record { Command::StartRecording } else { Command::Copy });
            }
            vk::LEFT | vk::RIGHT | vk::UP | vk::DOWN => {
                let step = if shift { 10.0 } else { 1.0 };
                let (dx, dy) = match code {
                    vk::LEFT => (-step, 0.0),
                    vk::RIGHT => (step, 0.0),
                    vk::UP => (0.0, step),
                    _ => (0.0, -step),
                };
                let active = self.active;
                if self.screens[active].sel.nudge(dx, dy, option) {
                    self.refresh(active);
                }
            }
            vk::LEFT_BRACKET => self.set_stroke(self.stroke.saturating_sub(1)),
            vk::RIGHT_BRACKET => self.set_stroke(self.stroke + 1),
            _ => {}
        }
        None
    }
}

/// Routes mouse events from overlay views.
pub fn mouse(idx: usize, kind: MouseKind, p: Point, e: &NSEvent) {
    mouse_at(idx, kind, p, shift_held(e));
}

pub fn mouse_at(idx: usize, kind: MouseKind, p: Point, shift: bool) {
    let cmd = with(|s| s.mouse(idx, kind, p, shift)).flatten();
    if let Some(cmd) = cmd {
        command(cmd);
    }
}

pub fn right_click(_idx: usize) {
    // Right-click cancels, like Esc.
    close();
}

pub fn cursor_update(idx: usize, p: Point) {
    with(|s| s.cursor_for(idx, p));
}

pub fn flags_changed(idx: usize, e: &NSEvent) {
    let shift = shift_held(e);
    with(|s| {
        if s.draft.is_some() {
            let p = s.last_point;
            let _ = s.mouse(idx, MouseKind::Drag, p, shift);
        }
    });
}

/// Scroll changes stroke size (up = thicker). Trackpads accumulate.
pub fn scroll(e: &NSEvent) {
    let dy = e.scrollingDeltaY();
    let precise = e.hasPreciseScrollingDeltas();
    with(|s| {
        s.scroll_acc += if precise { dy } else { dy.signum() * 20.0 };
        if s.scroll_acc.abs() >= 18.0 {
            let up = s.scroll_acc > 0.0;
            s.scroll_acc = 0.0;
            let next = if up { s.stroke + 1 } else { s.stroke.saturating_sub(1) };
            s.set_stroke(next);
        }
    });
}

pub fn key_down(e: &NSEvent) -> bool {
    match with(|s| s.key(e)) {
        Some(Some(Command::DismissHud)) => true,
        Some(Some(cmd)) => {
            command(cmd);
            true
        }
        Some(None) => true,
        None => false,
    }
}

/// ⌘C / ⌘S / ⌘P / ⌘Z inside the overlay.
pub fn key_equivalent(e: &NSEvent) -> bool {
    let flags = e.modifierFlags();
    if !flags.contains(NSEventModifierFlags::Command) || with(|s| s.text.is_some()).unwrap_or(true) {
        return false;
    }
    let cmd = match e.keyCode() {
        vk::C => Command::Copy,
        vk::S => Command::Save,
        vk::P => Command::Print,
        vk::Z => Command::Undo,
        _ => return false,
    };
    command(cmd);
    true
}

/// Called by the shared text delegate.
pub fn text_changed() {
    with(|s| {
        if let Some(t) = &s.text {
            resize_text_field(&t.field);
        }
    });
}

/// Developer self-test: type into the inline text editor.
pub fn set_text_for_test(s: &str) {
    with(|ss| {
        if let Some(t) = &ss.text {
            t.field.setStringValue(&ns(s));
            resize_text_field(&t.field);
        }
    });
}

pub fn text_finished(commit: bool) {
    with(|s| if commit { s.commit_text() } else { s.cancel_text() });
}

fn resize_text_field(field: &NSTextField) {
    let text = field.stringValue().to_string();
    let size = field.font().map(|f| f.pointSize()).unwrap_or(22.0);
    let (w, h) = render::text_size(&text, size);
    let f = field.frame();
    let top = f.origin.y + f.size.height;
    field.setFrame(cg_rect(Rect::new(f.origin.x, top - h - 2.0, w + 24.0, h + 2.0)));
}

/// Draws markup for the active display, clipped to the selection.
pub fn draw_annotations(idx: usize, ctx: &CGContext, _dirty: NSRect) {
    let ok = SESSION.with(|cell| {
        let Ok(guard) = cell.try_borrow() else { return false };
        let Some(s) = guard.as_ref() else { return true };
        if idx != s.active {
            return true;
        }
        let sc = &s.screens[idx];
        let Some(r) = sc.sel.rect else { return true };
        CGContext::save_g_state(Some(ctx));
        CGContext::clip_to_rect(Some(ctx), cg_rect(r));
        let src = Source { image: &sc.image, view_h: sc.bounds.h, scale: sc.display.scale };
        render::draw_shapes(ctx, &s.doc.shapes, &src);
        if let Some(shape) = s.draft.as_ref().and_then(|d| d.shape(s.doc.next_step())) {
            render::draw_shape(ctx, &shape, &src);
        }
        CGContext::restore_g_state(Some(ctx));
        true
    });
    if !ok {
        // Drawn while the session was busy; try again next turn of the loop.
        util::main_async(move || {
            with(|s| s.screens.get(idx).map(|sc| sc.annot.setNeedsDisplay(true)));
        });
    }
}

/// What the overlay hands to export/record when it finishes.
pub struct Finished {
    pub image: CFRetained<CGImage>,
    pub px: (u32, u32),
    pub display_name: String,
    pub front_app: Option<String>,
    /// Selection in AppKit global points (for pinning in place).
    pub global_rect: Rect,
}

fn finish_image() -> Option<Finished> {
    with(|s| {
        s.commit_text();
        let sc = s.screen();
        let r = sc.sel.rect?;
        let image = export::compose(&sc.image, r, sc.bounds.h, sc.display.scale, &s.doc.shapes)?;
        Some(Finished {
            px: r.pixel_size(sc.display.scale),
            image,
            display_name: sc.display.name.clone(),
            front_app: s.front_app.clone(),
            global_rect: r.translate(sc.display.frame.x, sc.display.frame.y),
        })
    })
    .flatten()
}

/// Region to record, taken from the active selection.
pub struct RecordTarget {
    pub display_id: u32,
    /// Selection in display-local points, top-left origin (SCK `sourceRect`).
    pub source_rect: Rect,
    /// Selection in AppKit global points.
    pub global_rect: Rect,
    pub scale: f64,
    pub display_name: String,
    pub front_app: Option<String>,
}

fn record_target() -> Option<RecordTarget> {
    with(|s| {
        let sc = s.screen();
        let r = sc.sel.rect?;
        Some(RecordTarget {
            display_id: sc.display.id,
            source_rect: Rect::new(r.x, sc.bounds.h - r.y - r.h, r.w, r.h),
            global_rect: r.translate(sc.display.frame.x, sc.display.frame.y),
            scale: sc.display.scale,
            display_name: sc.display.name.clone(),
            front_app: s.front_app.clone(),
        })
    })
    .flatten()
}

/// Overlay commands from toolbars, keys and menus.
pub fn command(cmd: Command) {
    match cmd {
        Command::Close => close(),
        Command::Tool(i) => {
            with(|s| s.select_tool(Tool::from_index(i)));
        }
        Command::Color(i) => {
            with(|s| {
                if let Some(c) = PALETTE.get(i as usize) {
                    s.set_color(*c, Some(i as usize));
                }
                s.toolbars.toggle_swatches();
            });
        }
        Command::ToggleSwatches => {
            with(|s| s.toolbars.toggle_swatches());
        }
        Command::CustomColor => {
            let mtm = mtm();
            with(|s| {
                if s.toolbars.swatches_shown() {
                    s.toolbars.toggle_swatches();
                }
            });
            let panel = NSColorPanel::sharedColorPanel(mtm);
            panel.setLevel(objc2_app_kit::NSScreenSaverWindowLevel + 1);
            let target = ui::controller(mtm);
            unsafe {
                panel.setTarget(Some(&target));
                panel.setAction(Some(objc2::sel!(colorChanged:)));
            }
            panel.setShowsAlpha(false);
            panel.orderFront(None);
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
                    s.redraw_annotations();
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
                export::save_with_panel(f);
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
            with(|s| s.toolbars.set_record_toggles(st.rec_mic, st.rec_system_audio, st.rec_webcam));
            if cmd == Command::ToggleMic && st.rec_mic {
                permissions::ask_microphone();
            }
            if cmd == Command::ToggleWebcam && st.rec_webcam {
                permissions::ask_camera();
            }
        }
        _ => {}
    }
}

/// From the system colour panel.
pub fn custom_color_changed(c: Rgba) {
    with(|s| s.set_color(c, None));
}
