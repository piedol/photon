//! Overlay toolbars: the vertical tool palette, the swatch strip, the action bar,
//! the record bar, and the size badge. Built once per session, placed around the
//! selection, flipping inward near screen edges.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBox, NSBoxType, NSButton, NSColor, NSFont, NSTextField, NSView, NSVisualEffectView,
    NSFontWeightMedium,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::annotate::model::{Rgba, Tool, PALETTE, PALETTE_NAMES};
use crate::capture::cg_rect;
use crate::commands::Command;
use crate::geometry::Rect;
use crate::overlay::chrome;
use crate::ui;
use crate::util::ns;

const BTN: f64 = 28.0;
const PAD: f64 = 4.0;
const GAP: f64 = 8.0;
const EDGE: f64 = 4.0;
const RADIUS: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Screenshot,
    Record,
}

pub struct Toolbars {
    pub tools: Retained<NSVisualEffectView>,
    tool_buttons: Vec<Retained<NSButton>>,
    color_button: Retained<NSButton>,
    stroke_button: Retained<NSButton>,
    pub swatches: Retained<NSVisualEffectView>,
    swatch_buttons: Vec<Retained<NSButton>>,
    pub actions: Retained<NSVisualEffectView>,
    pub record_bar: Retained<NSVisualEffectView>,
    mic: Retained<NSButton>,
    audio: Retained<NSButton>,
    cam: Retained<NSButton>,
    pub badge: Retained<NSVisualEffectView>,
    badge_label: Retained<NSTextField>,
    mode: Mode,
}

fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

fn divider(mtm: MainThreadMarker, f: NSRect) -> Retained<NSBox> {
    let b = NSBox::initWithFrame(NSBox::alloc(mtm), f);
    b.setBoxType(NSBoxType::Custom);
    b.setBorderWidth(0.0);
    b.setFillColor(&NSColor::colorWithWhite_alpha(1.0, 0.16));
    b
}

fn rounded_bg(b: &NSButton) {
    b.setWantsLayer(true);
    if let Some(l) = b.layer() {
        l.setCornerRadius(6.0);
    }
}

fn set_selected(b: &NSButton, on: bool) {
    if let Some(l) = b.layer() {
        let c = if on { chrome::accent(0.95) } else { chrome::rgba(0.0, 0.0, 0.0, 0.0) };
        l.setBackgroundColor(Some(&c));
    }
}

fn swatch(mtm: MainThreadMarker, f: NSRect, col: Rgba, cmd: Command, tip: &str) -> Retained<NSButton> {
    let b = ui::text_button(mtm, "", cmd);
    b.setBordered(false);
    b.setFrame(f);
    b.setToolTip(Some(&ns(tip)));
    b.setWantsLayer(true);
    if let Some(l) = b.layer() {
        l.setCornerRadius(f.size.width / 2.0);
        l.setBackgroundColor(Some(&chrome::rgba(col.0, col.1, col.2, 1.0)));
        l.setBorderWidth(1.5);
        l.setBorderColor(Some(&chrome::rgba(1.0, 1.0, 1.0, 0.85)));
    }
    b
}

impl Toolbars {
    pub fn new(mtm: MainThreadMarker, mode: Mode) -> Toolbars {
        // Vertical tool palette.
        let n_slots = Tool::ALL.len() + 3; // + colour, stroke, undo
        let h = PAD * 2.0 + BTN * n_slots as f64 + GAP;
        let w = BTN + PAD * 2.0;
        let tools = ui::hud_surface(mtm, frame(0.0, 0.0, w, h), RADIUS, true);
        let mut y = h - PAD - BTN;
        let mut tool_buttons = Vec::new();
        for t in Tool::ALL {
            let b = ui::icon_button(mtm, t.symbol(), t.tooltip(), Command::Tool(t.index()));
            b.setFrame(frame(PAD, y, BTN, BTN));
            rounded_bg(&b);
            tools.addSubview(&b);
            tool_buttons.push(b);
            y -= BTN;
        }
        tools.addSubview(&divider(mtm, frame(PAD + 4.0, y + BTN - 1.0 - GAP / 2.0 + 4.0, BTN - 8.0, 1.0)));
        y -= GAP;
        let color_button = swatch(mtm, frame(PAD + 6.0, y + 6.0, BTN - 12.0, BTN - 12.0), PALETTE[0], Command::CustomColor, "Color");
        color_button.setTag(Command::ToggleSwatches.tag());
        tools.addSubview(&color_button);
        y -= BTN;
        let stroke_button = ui::icon_button(mtm, "circle.fill", "Stroke size  ([ ] or scroll)", Command::CycleStroke);
        stroke_button.setFrame(frame(PAD, y, BTN, BTN));
        tools.addSubview(&stroke_button);
        y -= BTN;
        let undo = ui::icon_button(mtm, "arrow.uturn.backward", "Undo  (⌘Z)", Command::Undo);
        undo.setFrame(frame(PAD, y, BTN, BTN));
        tools.addSubview(&undo);

        // Swatch strip.
        let sw = 20.0;
        let sw_w = PAD * 2.0 + (sw + 6.0) * (PALETTE.len() + 1) as f64 - 6.0 + 4.0;
        let swatches = ui::hud_surface(mtm, frame(0.0, 0.0, sw_w, sw + PAD * 2.0 + 4.0), RADIUS, true);
        let mut swatch_buttons = Vec::new();
        let mut x = PAD + 2.0;
        for (i, col) in PALETTE.iter().enumerate() {
            let b = swatch(mtm, frame(x, PAD + 2.0, sw, sw), *col, Command::Color(i as u8), PALETTE_NAMES[i]);
            swatches.addSubview(&b);
            swatch_buttons.push(b);
            x += sw + 6.0;
        }
        let custom = ui::icon_button(mtm, "eyedropper", "Custom color…", Command::CustomColor);
        custom.setFrame(frame(x - 2.0, PAD, sw + 4.0, sw + 4.0));
        swatches.addSubview(&custom);
        swatches.setHidden(true);

        // Action bar.
        let items: [(&str, &str, Command); 5] = [
            ("record.circle", "Record this area", Command::RecordSelection),
            ("pin", "Pin to screen", Command::Pin),
            ("printer", "Print  (⌘P)", Command::Print),
            ("square.and.arrow.down", "Save  (⌘S)", Command::Save),
            ("doc.on.doc", "Copy  (⌘C or ↩)", Command::Copy),
        ];
        let aw = PAD * 2.0 + BTN * (items.len() + 1) as f64 + GAP;
        let actions = ui::hud_surface(mtm, frame(0.0, 0.0, aw, BTN + PAD * 2.0), RADIUS, true);
        let mut x = PAD;
        for (sym, tip, cmd) in items {
            let b = ui::icon_button(mtm, sym, tip, cmd);
            b.setFrame(frame(x, PAD, BTN, BTN));
            actions.addSubview(&b);
            x += BTN;
        }
        actions.addSubview(&divider(mtm, frame(x + GAP / 2.0 - 0.5, PAD + 6.0, 1.0, BTN - 12.0)));
        x += GAP;
        let close = ui::icon_button(mtm, "xmark", "Close  (esc)", Command::Close);
        close.setFrame(frame(x, PAD, BTN, BTN));
        actions.addSubview(&close);

        // Record bar.
        let start_w = 104.0;
        let rw = PAD * 2.0 + BTN * 4.0 + GAP * 2.0 + start_w;
        let record_bar = ui::hud_surface(mtm, frame(0.0, 0.0, rw, BTN + PAD * 2.0), RADIUS, true);
        let mic = ui::icon_button(mtm, "mic", "Microphone", Command::ToggleMic);
        let audio = ui::icon_button(mtm, "speaker.wave.2", "System audio", Command::ToggleSystemAudio);
        let cam = ui::icon_button(mtm, "video", "Webcam bubble", Command::ToggleWebcam);
        let mut x = PAD;
        for b in [&mic, &audio, &cam] {
            b.setFrame(frame(x, PAD, BTN, BTN));
            record_bar.addSubview(b);
            x += BTN;
        }
        record_bar.addSubview(&divider(mtm, frame(x + GAP / 2.0 - 0.5, PAD + 6.0, 1.0, BTN - 12.0)));
        x += GAP;
        let start = ui::text_button(mtm, "Record", Command::StartRecording);
        start.setImage(ui::symbol("record.circle.fill", 13.0).as_deref());
        start.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
        start.setBezelColor(Some(&NSColor::systemRedColor()));
        start.setKeyEquivalent(&ns("\r"));
        start.setFrame(frame(x, PAD - 1.0, start_w, BTN + 2.0));
        record_bar.addSubview(&start);
        x += start_w + GAP;
        let close = ui::icon_button(mtm, "xmark", "Cancel  (esc)", Command::Close);
        close.setFrame(frame(x, PAD, BTN, BTN));
        record_bar.addSubview(&close);

        // Size badge.
        let badge = ui::hud_surface(mtm, frame(0.0, 0.0, 92.0, 22.0), 6.0, true);
        let badge_label = NSTextField::labelWithString(&ns(""), mtm);
        badge_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium })));
        badge_label.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.92)));
        badge_label.setFrame(frame(7.0, 3.0, 80.0, 16.0));
        badge.addSubview(&badge_label);

        let tb = Toolbars {
            tools, tool_buttons, color_button, stroke_button, swatches, swatch_buttons,
            actions, record_bar, mic, audio, cam, badge, badge_label, mode,
        };
        tb.hide_all();
        tb
    }

    pub fn views(&self) -> [&NSView; 5] {
        [&self.tools, &self.swatches, &self.actions, &self.record_bar, &self.badge]
    }

    /// Move every toolbar into `parent` (the active display's overlay view).
    pub fn attach(&self, parent: &NSView) {
        for v in self.views() {
            if unsafe { v.superview() }.as_deref() != Some(parent) {
                v.removeFromSuperview();
                parent.addSubview(v);
            }
        }
    }

    pub fn hide_all(&self) {
        for v in self.views() {
            v.setHidden(true);
        }
    }

    pub fn set_tool(&self, tool: Option<Tool>) {
        for (i, b) in self.tool_buttons.iter().enumerate() {
            set_selected(b, tool.map(|t| t.index() as usize) == Some(i));
        }
    }

    pub fn set_color(&self, col: Rgba, index: Option<usize>) {
        if let Some(l) = self.color_button.layer() {
            l.setBackgroundColor(Some(&chrome::rgba(col.0, col.1, col.2, 1.0)));
        }
        for (i, b) in self.swatch_buttons.iter().enumerate() {
            if let Some(l) = b.layer() {
                let on = index == Some(i);
                l.setBorderWidth(if on { 2.5 } else { 1.5 });
                let c = if on { chrome::accent(1.0) } else { chrome::rgba(1.0, 1.0, 1.0, 0.85) };
                l.setBorderColor(Some(&c));
            }
        }
    }

    pub fn set_stroke(&self, size: usize) {
        let pts = [6.0, 9.0, 13.0][size.min(2)];
        self.stroke_button.setImage(ui::symbol("circle.fill", pts).as_deref());
    }

    pub fn set_record_toggles(&self, mic: bool, audio: bool, cam: bool) {
        let set = |b: &NSButton, on: bool, sym_on: &str, sym_off: &str| {
            b.setImage(ui::symbol(if on { sym_on } else { sym_off }, 15.0).as_deref());
            b.setContentTintColor(Some(&NSColor::colorWithWhite_alpha(1.0, if on { 0.95 } else { 0.45 })));
        };
        set(&self.mic, mic, "mic.fill", "mic.slash");
        set(&self.audio, audio, "speaker.wave.2.fill", "speaker.slash");
        set(&self.cam, cam, "video.fill", "video.slash");
    }

    pub fn toggle_swatches(&self) -> bool {
        let show = self.swatches.isHidden();
        self.swatches.setHidden(!show);
        show
    }

    pub fn swatches_shown(&self) -> bool {
        !self.swatches.isHidden()
    }

    /// Place everything around `sel` inside `bounds` (both view-local).
    /// `dragging` hides the bars while the selection is being changed.
    pub fn layout(&self, sel: Option<Rect>, bounds: Rect, px: (u32, u32), dragging: bool, label: Option<&str>) {
        let Some(sel) = sel else {
            self.hide_all();
            return;
        };
        // Badge: above the top-left corner, or inside it when there's no room.
        let text = match label {
            Some(l) => format!("{l} · {} × {}", px.0, px.1),
            None => format!("{} × {}", px.0, px.1),
        };
        self.badge_label.setStringValue(&ns(&text));
        let bw = 18.0 + 6.6 * text.chars().count() as f64;
        let mut b = Rect::new(sel.x, sel.max_y() + 6.0, bw, 22.0);
        if b.max_y() > bounds.max_y() - EDGE {
            b = Rect::new(sel.x + 6.0, sel.max_y() - 28.0, bw, 22.0);
        }
        self.badge.setFrame(cg_rect(b.clamp_inside(&bounds)));
        self.badge_label.setFrame(frame(7.0, 3.0, bw - 10.0, 16.0));
        self.badge.setHidden(false);

        if dragging {
            for v in [&self.tools, &self.swatches, &self.actions, &self.record_bar] {
                v.setHidden(true);
            }
            return;
        }

        // Bottom bar (actions or record bar): below, else above, else inside.
        let bar: &NSVisualEffectView = if self.mode == Mode::Record { &self.record_bar } else { &self.actions };
        let bs = bar.frame().size;
        let mut by = sel.y - GAP - bs.height;
        if by < bounds.y + EDGE {
            by = sel.max_y() + GAP;
            if by + bs.height > bounds.max_y() - EDGE {
                by = sel.y + GAP;
            }
        }
        let bar_r = Rect::new(sel.max_x() - bs.width, by, bs.width, bs.height).clamp_inside(&bounds.inset(EDGE));
        bar.setFrame(cg_rect(bar_r));
        bar.setHidden(false);

        if self.mode == Mode::Screenshot {
            // Tool palette: right of the selection, else left, else inside.
            let ts = self.tools.frame().size;
            let mut tx = sel.max_x() + GAP;
            let mut outside_right = true;
            if tx + ts.width > bounds.max_x() - EDGE {
                tx = sel.x - GAP - ts.width;
                outside_right = false;
                if tx < bounds.x + EDGE {
                    tx = sel.max_x() - GAP - ts.width;
                }
            }
            let tools_r = Rect::new(tx, sel.y, ts.width, ts.height).clamp_inside(&bounds.inset(EDGE));
            self.tools.setFrame(cg_rect(tools_r));
            self.tools.setHidden(false);

            // Swatches beside the colour button.
            let ss = self.swatches.frame().size;
            let color_y = tools_r.y + PAD + BTN * 2.0 + (BTN - ss.height) / 2.0;
            let sx = if outside_right && tools_r.max_x() + GAP + ss.width <= bounds.max_x() - EDGE {
                tools_r.max_x() + GAP
            } else {
                tools_r.x - GAP - ss.width
            };
            let sw_r = Rect::new(sx, color_y, ss.width, ss.height).clamp_inside(&bounds.inset(EDGE));
            self.swatches.setFrame(cg_rect(sw_r));
        }
    }
}
