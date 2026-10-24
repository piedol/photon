//! Annotation document: vector shapes in view-local points, plus undo. Pure logic.

use crate::geometry::{constrain_45, constrain_square, pt, Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    const fn hex(v: u32) -> Rgba {
        Rgba(((v >> 16) & 0xff) as f64 / 255.0, ((v >> 8) & 0xff) as f64 / 255.0, (v & 0xff) as f64 / 255.0, 1.0)
    }
    /// Perceived luminance, used to pick a readable number colour on step badges.
    pub fn luminance(&self) -> f64 {
        0.2126 * self.0 + 0.7152 * self.1 + 0.0722 * self.2
    }
}

pub const PALETTE: [Rgba; 8] = [
    Rgba::hex(0xFF3B30), // red
    Rgba::hex(0xFF9500), // orange
    Rgba::hex(0xFFD60A), // yellow
    Rgba::hex(0x30D158), // green
    Rgba::hex(0x0A84FF), // blue
    Rgba::hex(0xBF5AF2), // purple
    Rgba::hex(0x1C1C1E), // black
    Rgba::hex(0xFFFFFF), // white
];
pub const PALETTE_NAMES: [&str; 8] = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Black", "White"];

/// Three stroke sizes: thin, medium, thick.
pub const STROKES: [f64; 3] = [2.0, 4.0, 7.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Marker,
    Text,
    Blur,
    Step,
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Pen, Tool::Line, Tool::Arrow, Tool::Rect, Tool::Ellipse, Tool::Marker, Tool::Text, Tool::Blur, Tool::Step,
    ];

    pub fn index(self) -> u8 {
        Tool::ALL.iter().position(|t| *t == self).unwrap() as u8
    }
    pub fn from_index(i: u8) -> Option<Tool> {
        Tool::ALL.get(i as usize).copied()
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Tool::Pen => "pencil.tip",
            Tool::Line => "line.diagonal",
            Tool::Arrow => "arrow.up.right",
            Tool::Rect => "rectangle",
            Tool::Ellipse => "circle",
            Tool::Marker => "highlighter",
            Tool::Text => "textformat",
            Tool::Blur => "checkerboard.rectangle",
            Tool::Step => "1.circle",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Tool::Pen => "Pen",
            Tool::Line => "Line  (⇧ for 45°)",
            Tool::Arrow => "Arrow  (⇧ for 45°)",
            Tool::Rect => "Rectangle  (⇧ for square)",
            Tool::Ellipse => "Ellipse  (⇧ for circle)",
            Tool::Marker => "Marker",
            Tool::Text => "Text",
            Tool::Blur => "Pixelate to hide",
            Tool::Step => "Numbered step",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: Rgba,
    /// Index into `STROKES`.
    pub size: usize,
}

impl Style {
    pub fn width(&self) -> f64 {
        STROKES[self.size.min(2)]
    }
    pub fn marker_width(&self) -> f64 {
        [12.0, 18.0, 28.0][self.size.min(2)]
    }
    pub fn font_size(&self) -> f64 {
        [16.0, 22.0, 32.0][self.size.min(2)]
    }
    pub fn step_radius(&self) -> f64 {
        [11.0, 14.0, 18.0][self.size.min(2)]
    }
    /// Pixel block size for pixelation, in points.
    pub fn blur_block(&self) -> f64 {
        [8.0, 12.0, 18.0][self.size.min(2)]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Pen { pts: Vec<Point>, style: Style },
    Marker { pts: Vec<Point>, style: Style },
    Line { a: Point, b: Point, style: Style },
    Arrow { a: Point, b: Point, style: Style },
    Rect { r: Rect, style: Style },
    Ellipse { r: Rect, style: Style },
    Text { origin: Point, text: String, style: Style },
    Blur { r: Rect, style: Style },
    Step { center: Point, n: u32, style: Style },
}

/// A shape being dragged out.
#[derive(Clone, Debug)]
pub struct Draft {
    tool: Tool,
    style: Style,
    start: Point,
    end: Point,
    pts: Vec<Point>,
}

impl Draft {
    pub fn begin(tool: Tool, style: Style, p: Point) -> Draft {
        Draft { tool, style, start: p, end: p, pts: vec![p] }
    }

    pub fn update(&mut self, p: Point, shift: bool) {
        match self.tool {
            Tool::Pen | Tool::Marker => {
                if shift {
                    // Shift draws a straight stroke from the start point.
                    self.pts = vec![self.start, constrain_45(self.start, p)];
                } else if self.pts.last().is_none_or(|l| l.dist(p) >= 0.75) {
                    self.pts.push(p);
                }
            }
            Tool::Line | Tool::Arrow => self.end = if shift { constrain_45(self.start, p) } else { p },
            Tool::Rect | Tool::Ellipse | Tool::Blur => {
                self.end = if shift { constrain_square(self.start, p) } else { p }
            }
            Tool::Text | Tool::Step => self.end = p,
        }
    }

    /// The shape as it currently stands (for live drawing).
    pub fn shape(&self, next_step: u32) -> Option<Shape> {
        let style = self.style;
        let r = Rect::from_points(self.start, self.end);
        Some(match self.tool {
            Tool::Pen => Shape::Pen { pts: self.pts.clone(), style },
            Tool::Marker => Shape::Marker { pts: self.pts.clone(), style },
            Tool::Line => Shape::Line { a: self.start, b: self.end, style },
            Tool::Arrow => Shape::Arrow { a: self.start, b: self.end, style },
            Tool::Rect => Shape::Rect { r, style },
            Tool::Ellipse => Shape::Ellipse { r, style },
            Tool::Blur => Shape::Blur { r, style },
            Tool::Step => Shape::Step { center: self.end, n: next_step, style },
            Tool::Text => return None,
        })
    }

    /// Finish the drag. Tiny accidental shapes are dropped.
    pub fn finish(self, next_step: u32) -> Option<Shape> {
        let len = self.start.dist(self.end);
        let r = Rect::from_points(self.start, self.end);
        let ok = match self.tool {
            Tool::Pen | Tool::Marker => !self.pts.is_empty(),
            Tool::Line | Tool::Arrow => len >= 3.0,
            Tool::Rect | Tool::Ellipse | Tool::Blur => r.w >= 3.0 && r.h >= 3.0,
            Tool::Step => true,
            Tool::Text => false,
        };
        if !ok {
            return None;
        }
        let mut shape = self.shape(next_step)?;
        if let Shape::Pen { pts, .. } | Shape::Marker { pts, .. } = &mut shape
            && pts.len() == 1 {
                // A click draws a dot.
                let p = pts[0];
                pts.push(pt(p.x + 0.01, p.y));
            }
        Some(shape)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Document {
    pub shapes: Vec<Shape>,
}

impl Document {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    pub fn push(&mut self, s: Shape) {
        self.shapes.push(s);
    }

    pub fn undo(&mut self) -> Option<Shape> {
        self.shapes.pop()
    }

    /// Number for the next step badge: one more than the highest so far.
    pub fn next_step(&self) -> u32 {
        self.shapes
            .iter()
            .filter_map(|s| if let Shape::Step { n, .. } = s { Some(*n) } else { None })
            .max()
            .unwrap_or(0)
            + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: Style = Style { color: PALETTE[0], size: 1 };

    #[test]
    fn rect_draft_with_shift_is_square() {
        let mut d = Draft::begin(Tool::Rect, STYLE, pt(0.0, 0.0));
        d.update(pt(30.0, 10.0), true);
        let Some(Shape::Rect { r, .. }) = d.finish(1) else { panic!() };
        assert_eq!((r.w, r.h), (30.0, 30.0));
    }

    #[test]
    fn tiny_shapes_dropped_and_clicks_make_dots() {
        let mut d = Draft::begin(Tool::Arrow, STYLE, pt(0.0, 0.0));
        d.update(pt(1.0, 1.0), false);
        assert!(d.finish(1).is_none());
        let d = Draft::begin(Tool::Pen, STYLE, pt(5.0, 5.0));
        let Some(Shape::Pen { pts, .. }) = d.finish(1) else { panic!() };
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn steps_count_up_and_undo() {
        let mut doc = Document::default();
        assert_eq!(doc.next_step(), 1);
        for _ in 0..3 {
            let n = doc.next_step();
            doc.push(Draft::begin(Tool::Step, STYLE, pt(1.0, 1.0)).finish(n).unwrap());
        }
        assert_eq!(doc.next_step(), 4);
        doc.undo();
        assert_eq!(doc.next_step(), 3);
    }

    #[test]
    fn shift_line_snaps() {
        let mut d = Draft::begin(Tool::Line, STYLE, pt(0.0, 0.0));
        d.update(pt(50.0, 3.0), true);
        let Some(Shape::Line { b, .. }) = d.finish(1) else { panic!() };
        assert!(b.y.abs() < 1e-9);
    }

    #[test]
    fn tool_indices_round_trip() {
        for t in Tool::ALL {
            assert_eq!(Tool::from_index(t.index()), Some(t));
        }
    }
}
