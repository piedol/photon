//! Direct2D / DirectWrite / WIC plumbing shared by every surface, plus
//! `Layered`, a per-pixel-alpha window painted with Direct2D.

use std::cell::OnceCell;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_LINE_JOIN_ROUND, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::w;
use windows_numerics::Vector2;

thread_local! {
    static FACTORY: OnceCell<ID2D1Factory> = const { OnceCell::new() };
    static DWRITE: OnceCell<IDWriteFactory> = const { OnceCell::new() };
    static WIC: OnceCell<IWICImagingFactory> = const { OnceCell::new() };
}

pub fn factory() -> ID2D1Factory {
    FACTORY.with(|f| {
        f.get_or_init(|| unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("Direct2D") })
            .clone()
    })
}

pub fn dwrite() -> IDWriteFactory {
    DWRITE.with(|f| f.get_or_init(|| unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DirectWrite") }).clone())
}

pub fn wic() -> IWICImagingFactory {
    WIC.with(|f| {
        f.get_or_init(|| unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).expect("WIC") })
            .clone()
    })
}

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub fn v2(x: f64, y: f64) -> Vector2 {
    Vector2 { X: x as f32, Y: y as f32 }
}

pub fn pt(x: f64, y: f64) -> Vector2 {
    Vector2 { X: x as f32, Y: y as f32 }
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> D2D_RECT_F {
    D2D_RECT_F { left: x as f32, top: y as f32, right: (x + w) as f32, bottom: (y + h) as f32 }
}

pub fn rr(x: f64, y: f64, w: f64, h: f64, radius: f64) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: radius as f32, radiusY: radius as f32 }
}

pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: pt(cx, cy), radiusX: rx as f32, radiusY: ry as f32 }
}

pub fn brush(rt: &ID2D1RenderTarget, c: D2D1_COLOR_F) -> ID2D1SolidColorBrush {
    unsafe { rt.CreateSolidColorBrush(&c, None).expect("brush") }
}

pub fn round_stroke() -> ID2D1StrokeStyle {
    let props = D2D1_STROKE_STYLE_PROPERTIES {
        startCap: D2D1_CAP_STYLE_ROUND,
        endCap: D2D1_CAP_STYLE_ROUND,
        dashCap: D2D1_CAP_STYLE_ROUND,
        lineJoin: D2D1_LINE_JOIN_ROUND,
        miterLimit: 10.0,
        ..Default::default()
    };
    unsafe { factory().CreateStrokeStyle(&props, None).expect("stroke") }
}

pub fn dashed_stroke(dash: &[f32]) -> ID2D1StrokeStyle {
    let props = D2D1_STROKE_STYLE_PROPERTIES {
        dashStyle: windows::Win32::Graphics::Direct2D::D2D1_DASH_STYLE_CUSTOM,
        miterLimit: 10.0,
        ..Default::default()
    };
    unsafe { factory().CreateStrokeStyle(&props, Some(dash)).expect("stroke") }
}

/// Build a path from a list of points (polyline). `closed` fills/closes it.
pub fn polyline(points: &[(f64, f64)], closed: bool) -> Option<ID2D1PathGeometry> {
    let (first, rest) = points.split_first()?;
    unsafe {
        let geo = factory().CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        sink.BeginFigure(pt(first.0, first.1), if closed { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
        for p in rest {
            sink.AddLine(pt(p.0, p.1));
        }
        sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
        sink.Close().ok()?;
        Some(geo)
    }
}

pub const WEIGHT_REGULAR: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT(400);
pub const WEIGHT_MEDIUM: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT(500);
pub const WEIGHT_SEMIBOLD: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT(600);
pub const WEIGHT_BOLD: DWRITE_FONT_WEIGHT = DWRITE_FONT_WEIGHT(700);

pub fn text_format(size: f64, weight: DWRITE_FONT_WEIGHT) -> IDWriteTextFormat {
    unsafe {
        dwrite()
            .CreateTextFormat(
                w!("Segoe UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size as f32,
                w!("en-us"),
            )
            .expect("text format")
    }
}

pub fn layout(text: &str, size: f64, weight: DWRITE_FONT_WEIGHT) -> IDWriteTextLayout {
    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe { dwrite().CreateTextLayout(&wide, &text_format(size, weight), 10_000.0, 10_000.0).expect("layout") }
}

/// (width, height) of laid-out text.
pub fn measure(l: &IDWriteTextLayout) -> (f64, f64) {
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe {
        let _ = l.GetMetrics(&mut m);
    }
    (m.widthIncludingTrailingWhitespace as f64, m.height as f64)
}

pub fn draw_text(rt: &ID2D1RenderTarget, text: &str, x: f64, y: f64, size: f64, weight: DWRITE_FONT_WEIGHT, color: D2D1_COLOR_F) -> (f64, f64) {
    let l = layout(text, size, weight);
    let b = brush(rt, color);
    unsafe { rt.DrawTextLayout(v2(x, y), &l, &b, D2D1_DRAW_TEXT_OPTIONS_NONE) };
    measure(&l)
}

/// Text centred in a rect.
pub fn draw_text_centered(rt: &ID2D1RenderTarget, text: &str, r: D2D_RECT_F, size: f64, weight: DWRITE_FONT_WEIGHT, color: D2D1_COLOR_F) {
    let l = layout(text, size, weight);
    let (w, h) = measure(&l);
    let x = (r.left + r.right) as f64 / 2.0 - w / 2.0;
    let y = (r.top + r.bottom) as f64 / 2.0 - h / 2.0;
    let b = brush(rt, color);
    unsafe { rt.DrawTextLayout(v2(x, y), &l, &b, D2D1_DRAW_TEXT_OPTIONS_NONE) };
}

pub fn premultiplied_props() -> D2D1_RENDER_TARGET_PROPERTIES {
    D2D1_RENDER_TARGET_PROPERTIES {
        // Software rendering: keeps the GPU driver (tens of MB) out of the process.
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    }
}

/// A per-pixel-alpha (layered) window surface: paint with Direct2D into a DIB,
/// then hand it to `UpdateLayeredWindow`.
pub struct Layered {
    pub hwnd: HWND,
    w: i32,
    h: i32,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    rt: ID2D1DCRenderTarget,
}

impl Layered {
    pub fn new(hwnd: HWND, w: i32, h: i32) -> Layered {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w.max(1),
                    biHeight: -h.max(1),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
            let old = SelectObject(dc, bmp.into());
            let rt = factory().CreateDCRenderTarget(&premultiplied_props()).expect("dc target");
            Layered { hwnd, w, h, dc, bmp, old, rt }
        }
    }

    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    /// Paint and present at screen position (x, y).
    pub fn paint(&self, x: i32, y: i32, alpha: u8, f: impl FnOnce(&ID2D1RenderTarget)) {
        unsafe {
            let r = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
            if self.rt.BindDC(self.dc, &r).is_err() {
                return;
            }
            let rt: ID2D1RenderTarget = windows_core::Interface::cast(&self.rt).expect("rt");
            rt.BeginDraw();
            rt.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
            f(&rt);
            let _ = rt.EndDraw(None, None);
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
            let src = POINT { x: 0, y: 0 };
            let dst = POINT { x, y };
            let size = SIZE { cx: self.w, cy: self.h };
            let _ = UpdateLayeredWindow(self.hwnd, None, Some(&dst), Some(&size), Some(self.dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }
}

impl Drop for Layered {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
