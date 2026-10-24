//! Overlay toolbars, laid out like the Mac app: the vertical tool palette on the
//! selection's right edge, the action bar (or record bar) under its bottom-right
//! corner, the swatch strip beside the colour button, and the size badge.
//! Everything is drawn with Direct2D, icons included, so it looks the same on
//! Windows 10 and 11 at any DPI.

use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};

use crate::d2d::{self, rgba};
use crate::geometry::{pt, Point, Rect};
use crate::model::{Rgba, Tool, PALETTE, PALETTE_NAMES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Screenshot,
    Record,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Tool(Tool),
    Color,
    Stroke,
    Undo,
    Swatch(usize),
    Custom,
    Record,
    Pin,
    Print,
    Save,
    Copy,
    Close,
    Mic,
    Audio,
    Cam,
    Start,
}

impl Btn {
    pub fn tooltip(self) -> String {
        match self {
            Btn::Tool(t) => t.tooltip().replace('⇧', "Shift"),
            Btn::Color => "Color".into(),
            Btn::Stroke => "Stroke size  ([ ] or scroll)".into(),
            Btn::Undo => "Undo  (Ctrl+Z)".into(),
            Btn::Swatch(i) => PALETTE_NAMES[i].into(),
            Btn::Custom => "Custom color…".into(),
            Btn::Record => "Record this area".into(),
            Btn::Pin => "Pin to screen".into(),
            Btn::Print => "Print  (Ctrl+P)".into(),
            Btn::Save => "Save  (Ctrl+S)".into(),
            Btn::Copy => "Copy  (Ctrl+C or Enter)".into(),
            Btn::Close => "Close  (Esc)".into(),
            Btn::Mic => "Microphone".into(),
            Btn::Audio => "System audio".into(),
            Btn::Cam => "Webcam bubble".into(),
            Btn::Start => "Start recording  (Enter)".into(),
        }
    }
}

/// What the toolbars need to know to draw their state.
pub struct DrawState {
    pub tool: Option<Tool>,
    pub color: Rgba,
    pub color_index: Option<usize>,
    pub stroke: usize,
    pub mic: bool,
    pub audio: bool,
    pub cam: bool,
    pub hover: Option<Btn>,
}

pub struct Toolbars {
    pub mode: Mode,
    scale: f64,
    pub swatches_shown: bool,
    items: Vec<(Btn, Rect)>,
    panels: Vec<Rect>,
    badge: Option<(Rect, String)>,
}

const BTN: f64 = 30.0;
const PAD: f64 = 4.0;
const GAP: f64 = 8.0;
const EDGE: f64 = 6.0;
const RADIUS: f64 = 10.0;

impl Toolbars {
    pub fn new(mode: Mode, scale: f64) -> Toolbars {
        Toolbars { mode, scale, swatches_shown: false, items: Vec::new(), panels: Vec::new(), badge: None }
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.panels.clear();
        self.badge = None;
    }

    pub fn hit(&self, p: Point) -> Option<Btn> {
        self.items.iter().rev().find(|(_, r)| r.contains(p)).map(|(b, _)| *b)
    }

    pub fn contains(&self, p: Point) -> bool {
        self.panels.iter().any(|r| r.contains(p)) || self.badge.as_ref().is_some_and(|(r, _)| r.contains(p))
    }


    /// Place everything around `sel` (pixels, y-down) inside `bounds`.
    pub fn layout(&mut self, sel: Option<Rect>, bounds: Rect, px: (u32, u32), dragging: bool, label: Option<&str>) {
        self.clear();
        let Some(sel) = sel else { return };
        let s = self.scale;
        let (btn, pad, gap, edge) = (BTN * s, PAD * s, GAP * s, EDGE * s);
        let inner = bounds.inset(edge);

        // Size badge above the top-left corner, or inside when there's no room.
        let text = match label {
            Some(l) => format!("{l} · {} × {}", px.0, px.1),
            None => format!("{} × {}", px.0, px.1),
        };
        let (tw, _) = d2d::measure(&d2d::layout(&text, 12.0 * s, d2d::WEIGHT_MEDIUM));
        let bh = 24.0 * s;
        let bw = tw + 16.0 * s;
        let mut b = Rect::new(sel.x, sel.y - bh - 6.0 * s, bw, bh);
        if b.y < bounds.y + edge {
            b = Rect::new(sel.x + 6.0 * s, sel.y + 6.0 * s, bw, bh);
        }
        self.badge = Some((b.clamp_inside(&inner), text));
        if dragging {
            return;
        }

        // Bottom bar: below the selection, else above, else inside.
        let bar: Vec<Btn> = match self.mode {
            Mode::Screenshot => vec![Btn::Record, Btn::Pin, Btn::Print, Btn::Save, Btn::Copy, Btn::Close],
            Mode::Record => vec![Btn::Mic, Btn::Audio, Btn::Cam, Btn::Start, Btn::Close],
        };
        let start_w = 104.0 * s;
        let bar_w = pad * 2.0
            + bar.iter().map(|b| if *b == Btn::Start { start_w } else { btn }).sum::<f64>()
            + gap;
        let bar_h = btn + pad * 2.0;
        let mut by = sel.max_y() + gap;
        if by + bar_h > bounds.max_y() - edge {
            by = sel.y - gap - bar_h;
            if by < bounds.y + edge {
                by = sel.max_y() - gap - bar_h;
            }
        }
        let bar_r = Rect::new(sel.max_x() - bar_w, by, bar_w, bar_h).clamp_inside(&inner);
        self.panels.push(bar_r);
        let mut x = bar_r.x + pad;
        for (i, b) in bar.iter().enumerate() {
            let last = i == bar.len() - 1;
            if last {
                x += gap; // divider before Close
            }
            let w = if *b == Btn::Start { start_w } else { btn };
            self.items.push((*b, Rect::new(x, bar_r.y + pad, w, btn)));
            x += w;
        }

        if self.mode == Mode::Screenshot {
            // Tool palette: right of the selection, else left, else inside.
            let slots = Tool::ALL.len() + 3;
            let pal_w = btn + pad * 2.0;
            let pal_h = pad * 2.0 + btn * slots as f64 + gap;
            let mut tx = sel.max_x() + gap;
            let mut right = true;
            if tx + pal_w > bounds.max_x() - edge {
                tx = sel.x - gap - pal_w;
                right = false;
                if tx < bounds.x + edge {
                    tx = sel.max_x() - gap - pal_w;
                }
            }
            // Bottom-aligned with the selection, like the Mac.
            let pal = Rect::new(tx, sel.max_y() - pal_h, pal_w, pal_h).clamp_inside(&inner);
            self.panels.push(pal);
            let mut y = pal.y + pad;
            for t in Tool::ALL {
                self.items.push((Btn::Tool(t), Rect::new(pal.x + pad, y, btn, btn)));
                y += btn;
            }
            y += gap;
            let color_r = Rect::new(pal.x + pad, y, btn, btn);
            self.items.push((Btn::Color, color_r));
            y += btn;
            self.items.push((Btn::Stroke, Rect::new(pal.x + pad, y, btn, btn)));
            y += btn;
            self.items.push((Btn::Undo, Rect::new(pal.x + pad, y, btn, btn)));

            if self.swatches_shown {
                let sw = 22.0 * s;
                let n = PALETTE.len() + 1;
                let strip_w = pad * 2.0 + (sw + 6.0 * s) * n as f64 - 6.0 * s + 4.0 * s;
                let strip_h = sw + pad * 2.0 + 4.0 * s;
                let sx = if right && pal.max_x() + gap + strip_w <= bounds.max_x() - edge {
                    pal.max_x() + gap
                } else {
                    pal.x - gap - strip_w
                };
                let strip = Rect::new(sx, color_r.mid().y - strip_h / 2.0, strip_w, strip_h).clamp_inside(&inner);
                self.panels.push(strip);
                let mut x = strip.x + pad + 2.0 * s;
                for i in 0..PALETTE.len() {
                    self.items.push((Btn::Swatch(i), Rect::new(x, strip.y + pad + 2.0 * s, sw, sw)));
                    x += sw + 6.0 * s;
                }
                self.items.push((Btn::Custom, Rect::new(x, strip.y + pad + 2.0 * s, sw, sw)));
            }
        }
    }

    pub fn draw(&self, rt: &ID2D1RenderTarget, st: &DrawState) {
        let s = self.scale;
        let panel = d2d::brush(rt, rgba(0.13, 0.13, 0.15, 0.94));
        let edge = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.10));
        let fg = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.92));
        let dim = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.42));
        let hover_b = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.12));
        let accent = d2d::brush(rt, crate::overlay::accent(0.95));
        unsafe {
            for p in &self.panels {
                let r = d2d::rr(p.x, p.y, p.w, p.h, RADIUS * s);
                rt.FillRoundedRectangle(&r, &panel);
                rt.DrawRoundedRectangle(&r, &edge, 1.0, None);
            }
            if let Some((b, text)) = &self.badge {
                let r = d2d::rr(b.x, b.y, b.w, b.h, 6.0 * s);
                rt.FillRoundedRectangle(&r, &panel);
                d2d::draw_text_centered(rt, text, d2d::rect(b.x, b.y, b.w, b.h), 12.0 * s, d2d::WEIGHT_MEDIUM, rgba(1.0, 1.0, 1.0, 0.92));
            }
        }
        for (btn, r) in &self.items {
            let selected = matches!(btn, Btn::Tool(t) if Some(*t) == st.tool);
            let hovered = st.hover == Some(*btn);
            unsafe {
                if selected {
                    rt.FillRoundedRectangle(&d2d::rr(r.x + 2.0 * s, r.y + 2.0 * s, r.w - 4.0 * s, r.h - 4.0 * s, 6.0 * s), &accent);
                } else if hovered && !matches!(btn, Btn::Swatch(_) | Btn::Start) {
                    rt.FillRoundedRectangle(&d2d::rr(r.x + 2.0 * s, r.y + 2.0 * s, r.w - 4.0 * s, r.h - 4.0 * s, 6.0 * s), &hover_b);
                }
            }
            let c = r.mid();
            match btn {
                Btn::Swatch(i) => {
                    let col = PALETTE[*i];
                    swatch(rt, c, r.w / 2.0, col, st.color_index == Some(*i), s);
                }
                Btn::Color => swatch(rt, c, 8.0 * s, st.color, false, s),
                Btn::Start => {
                    let red = d2d::brush(rt, rgba(1.0, 0.27, 0.23, if hovered { 1.0 } else { 0.92 }));
                    unsafe {
                        rt.FillRoundedRectangle(&d2d::rr(r.x + 3.0 * s, r.y + 2.0 * s, r.w - 6.0 * s, r.h - 4.0 * s, 7.0 * s), &red);
                    }
                    let white = d2d::brush(rt, rgba(1.0, 1.0, 1.0, 1.0));
                    unsafe { rt.FillEllipse(&d2d::ellipse(r.x + 20.0 * s, c.y, 5.0 * s, 5.0 * s), &white) };
                    d2d::draw_text(rt, "Record", r.x + 32.0 * s, c.y - 9.5 * s, 13.0 * s, d2d::WEIGHT_SEMIBOLD, rgba(1.0, 1.0, 1.0, 1.0));
                }
                Btn::Mic => icon(rt, if st.mic { Icon::Mic } else { Icon::MicOff }, c, s, if st.mic { &fg } else { &dim }),
                Btn::Audio => icon(rt, if st.audio { Icon::Speaker } else { Icon::SpeakerOff }, c, s, if st.audio { &fg } else { &dim }),
                Btn::Cam => icon(rt, if st.cam { Icon::Camera } else { Icon::CameraOff }, c, s, if st.cam { &fg } else { &dim }),
                Btn::Stroke => {
                    let rad = [2.5, 4.0, 6.0][st.stroke.min(2)] * s;
                    unsafe { rt.FillEllipse(&d2d::ellipse(c.x, c.y, rad, rad), &fg) };
                }
                other => icon(rt, Icon::for_btn(*other), c, s, &fg),
            }
        }
        // Divider before Close in the bottom bar.
        if let Some((_, close)) = self.items.iter().find(|(b, _)| *b == Btn::Close) {
            let x = close.x - GAP * s / 2.0;
            unsafe {
                rt.DrawLine(d2d::v2(x, close.y + 6.0 * s), d2d::v2(x, close.max_y() - 6.0 * s), &edge, 1.0, None);
            }
        }
    }
}

fn swatch(rt: &ID2D1RenderTarget, c: Point, r: f64, col: Rgba, selected: bool, s: f64) {
    let fill = d2d::brush(rt, rgba(col.0 as f32, col.1 as f32, col.2 as f32, 1.0));
    let ring = if selected { d2d::brush(rt, crate::overlay::accent(1.0)) } else { d2d::brush(rt, rgba(1.0, 1.0, 1.0, 0.85)) };
    unsafe {
        rt.FillEllipse(&d2d::ellipse(c.x, c.y, r - 1.0 * s, r - 1.0 * s), &fill);
        rt.DrawEllipse(&d2d::ellipse(c.x, c.y, r - 1.0 * s, r - 1.0 * s), &ring, if selected { 2.5 * s } else { 1.5 } as f32, None);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Pen,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Marker,
    Text,
    Pixelate,
    Step,
    Undo,
    Record,
    Pin,
    Print,
    Save,
    Copy,
    Close,
    Mic,
    MicOff,
    Speaker,
    SpeakerOff,
    Camera,
    CameraOff,
    Custom,
    Folder,
    Play,
    Trim,
    Gif,
    Stop,
    Pause,
}

impl Icon {
    fn for_btn(b: Btn) -> Icon {
        match b {
            Btn::Tool(t) => match t {
                Tool::Pen => Icon::Pen,
                Tool::Line => Icon::Line,
                Tool::Arrow => Icon::Arrow,
                Tool::Rect => Icon::Rect,
                Tool::Ellipse => Icon::Ellipse,
                Tool::Marker => Icon::Marker,
                Tool::Text => Icon::Text,
                Tool::Blur => Icon::Pixelate,
                Tool::Step => Icon::Step,
            },
            Btn::Undo => Icon::Undo,
            Btn::Record => Icon::Record,
            Btn::Pin => Icon::Pin,
            Btn::Print => Icon::Print,
            Btn::Save => Icon::Save,
            Btn::Copy => Icon::Copy,
            Btn::Close => Icon::Close,
            Btn::Custom => Icon::Custom,
            _ => Icon::Close,
        }
    }
}

/// Draw a 16pt-ish monochrome icon centred at `c`.
pub fn icon(rt: &ID2D1RenderTarget, which: Icon, c: Point, s: f64, b: &ID2D1SolidColorBrush) {
    let k = s; // one "point" in pixels
    let w = (1.6 * k) as f32;
    let round = d2d::round_stroke();
    let line = |a: (f64, f64), z: (f64, f64)| unsafe {
        rt.DrawLine(d2d::v2(c.x + a.0 * k, c.y + a.1 * k), d2d::v2(c.x + z.0 * k, c.y + z.1 * k), b, w, &round);
    };
    let poly = |pts: &[(f64, f64)], closed: bool, fill: bool| {
        let abs: Vec<(f64, f64)> = pts.iter().map(|p| (c.x + p.0 * k, c.y + p.1 * k)).collect();
        if let Some(g) = d2d::polyline(&abs, closed) {
            unsafe {
                if fill {
                    rt.FillGeometry(&g, b, None);
                } else {
                    rt.DrawGeometry(&g, b, w, &round);
                }
            }
        }
    };
    let circle = |cx: f64, cy: f64, r: f64, fill: bool| unsafe {
        let e = d2d::ellipse(c.x + cx * k, c.y + cy * k, r * k, r * k);
        if fill { rt.FillEllipse(&e, b) } else { rt.DrawEllipse(&e, b, w, &round) }
    };
    let rrect = |x: f64, y: f64, ww: f64, hh: f64, r: f64, fill: bool| unsafe {
        let rr = d2d::rr(c.x + x * k, c.y + y * k, ww * k, hh * k, r * k);
        if fill { rt.FillRoundedRectangle(&rr, b) } else { rt.DrawRoundedRectangle(&rr, b, w, &round) }
    };
    let arc = |cx: f64, cy: f64, r: f64, a0: f64, a1: f64| {
        let n = 14;
        let pts: Vec<(f64, f64)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f64 / n as f64).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        poly(&pts, false, false);
    };
    match which {
        Icon::Pen => {
            poly(&[(-6.0, 6.0), (-6.0, 3.0), (4.0, -7.0), (7.0, -4.0), (-3.0, 6.0)], true, false);
            line((2.0, -5.0), (5.0, -2.0));
        }
        Icon::Line => line((-6.0, 6.0), (6.0, -6.0)),
        Icon::Arrow => {
            line((-6.0, 6.0), (6.0, -6.0));
            poly(&[(-1.0, -6.0), (6.0, -6.0), (6.0, 1.0)], false, false);
        }
        Icon::Rect => rrect(-7.0, -5.5, 14.0, 11.0, 1.5, false),
        Icon::Ellipse => circle(0.0, 0.0, 6.5, false),
        Icon::Marker => {
            poly(&[(-2.0, 4.0), (5.0, -3.0), (7.0, -1.0), (0.0, 6.0)], true, false);
            poly(&[(-2.0, 4.0), (0.0, 6.0), (-3.0, 7.0), (-5.0, 7.0)], true, true);
            line((-7.0, 7.5), (7.0, 7.5));
        }
        Icon::Text => {
            line((-6.0, -6.0), (6.0, -6.0));
            line((0.0, -6.0), (0.0, 7.0));
        }
        Icon::Pixelate => {
            for (ix, iy) in [(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)] {
                rrect(-7.0 + ix as f64 * 5.0, -7.0 + iy as f64 * 5.0, 4.2, 4.2, 0.6, true);
            }
            rrect(-7.5, -7.5, 15.0, 15.0, 1.5, false);
        }
        Icon::Step => {
            circle(0.0, 0.0, 7.5, false);
            line((-1.5, -3.0), (1.0, -4.5));
            line((1.0, -4.5), (1.0, 4.5));
        }
        Icon::Undo => {
            arc(1.0, 1.0, 6.0, 180.0, 390.0);
            poly(&[(-8.0, -2.0), (-5.0, 2.5), (-1.5, -1.5)], false, false);
        }
        Icon::Record => {
            circle(0.0, 0.0, 7.5, false);
            circle(0.0, 0.0, 4.0, true);
        }
        Icon::Pin => {
            poly(&[(-1.0, -7.5), (6.5, 0.0), (3.5, 1.0), (-0.5, 5.0), (-5.0, 0.5), (-1.0, -3.5)], true, false);
            line((-2.5, 2.5), (-7.0, 7.0));
        }
        Icon::Print => {
            rrect(-7.5, -2.5, 15.0, 7.5, 1.5, false);
            poly(&[(-4.0, -2.5), (-4.0, -7.0), (4.0, -7.0), (4.0, -2.5)], false, false);
            rrect(-4.0, 2.5, 8.0, 5.0, 0.5, false);
        }
        Icon::Save => {
            line((0.0, -7.0), (0.0, 3.0));
            poly(&[(-4.0, -1.0), (0.0, 3.0), (4.0, -1.0)], false, false);
            poly(&[(-7.0, 2.0), (-7.0, 7.0), (7.0, 7.0), (7.0, 2.0)], false, false);
        }
        Icon::Copy => {
            rrect(-6.5, -3.5, 9.0, 11.0, 1.5, false);
            poly(&[(-3.5, -3.5), (-3.5, -7.0), (6.5, -7.0), (6.5, 4.0), (2.5, 4.0)], false, false);
        }
        Icon::Close => {
            line((-5.5, -5.5), (5.5, 5.5));
            line((5.5, -5.5), (-5.5, 5.5));
        }
        Icon::Mic | Icon::MicOff => {
            rrect(-3.0, -8.0, 6.0, 10.0, 3.0, false);
            arc(0.0, -1.0, 5.5, 0.0, 180.0);
            line((0.0, 4.5), (0.0, 7.5));
            if which == Icon::MicOff {
                line((-6.5, -7.5), (6.5, 7.5));
            }
        }
        Icon::Speaker | Icon::SpeakerOff => {
            poly(&[(-7.0, -2.5), (-4.0, -2.5), (0.0, -6.5), (0.0, 6.5), (-4.0, 2.5), (-7.0, 2.5)], true, false);
            if which == Icon::Speaker {
                arc(0.5, 0.0, 3.5, -45.0, 45.0);
                arc(0.5, 0.0, 6.5, -50.0, 50.0);
            } else {
                line((3.0, -3.0), (7.0, 3.0));
                line((7.0, -3.0), (3.0, 3.0));
            }
        }
        Icon::Camera | Icon::CameraOff => {
            rrect(-7.5, -5.0, 10.5, 10.0, 2.0, false);
            poly(&[(3.0, -1.5), (7.5, -4.0), (7.5, 4.0), (3.0, 1.5)], true, false);
            if which == Icon::CameraOff {
                line((-7.0, -7.0), (7.0, 7.0));
            }
        }
        Icon::Custom => {
            circle(0.0, 0.0, 7.0, false);
            line((0.0, -3.5), (0.0, 3.5));
            line((-3.5, 0.0), (3.5, 0.0));
        }
        Icon::Folder => {
            poly(&[(-7.5, -5.0), (-2.5, -5.0), (-1.0, -3.0), (7.5, -3.0), (7.5, 6.0), (-7.5, 6.0)], true, false);
        }
        Icon::Play => {
            rrect(-7.5, -6.0, 15.0, 12.0, 2.0, false);
            poly(&[(-2.0, -3.0), (3.5, 0.0), (-2.0, 3.0)], true, true);
        }
        Icon::Trim => {
            rrect(-7.5, -4.0, 15.0, 8.0, 1.5, false);
            line((-3.5, -6.5), (-3.5, 6.5));
            line((3.5, -6.5), (3.5, 6.5));
        }
        Icon::Gif => {
            rrect(-7.5, -6.0, 11.0, 9.0, 1.5, false);
            rrect(-3.5, -3.0, 11.0, 9.0, 1.5, false);
        }
        Icon::Stop => rrect(-5.0, -5.0, 10.0, 10.0, 2.0, true),
        Icon::Pause => {
            rrect(-5.0, -6.0, 3.5, 12.0, 1.0, true);
            rrect(1.5, -6.0, 3.5, 12.0, 1.0, true);
        }
    }
}

/// Convenience for other surfaces: icon at a point with a given colour.
pub fn icon_colored(rt: &ID2D1RenderTarget, which: Icon, x: f64, y: f64, s: f64, col: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) {
    let b = d2d::brush(rt, col);
    icon(rt, which, pt(x, y), s, &b);
}
