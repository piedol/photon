//! Turning a selection plus markup into an image, and the copy / save / print actions.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT;
use windows::Win32::Graphics::Direct2D::{D2D1_RENDER_TARGET_PROPERTIES, ID2D1RenderTarget};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_BITFIELDS, BITMAPINFO, BITMAPINFOHEADER, BITMAPV5HEADER, BI_RGB, DIB_RGB_COLORS, GetDeviceCaps, HORZRES, LOGPIXELSX,
    SRCCOPY, StretchDIBits, VERTRES, DeleteDC, HDC,
};
use windows::Win32::Graphics::Imaging::{
    GUID_ContainerFormatJpeg, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat32bppPBGRA,
    IWICBitmapFrameEncode, WICBitmapCacheOnLoad, WICBitmapEncoderNoCache, WICRect,
};
use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream, IPropertyBag2};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_DIBV5;
use windows::Win32::UI::Controls::Dialogs::{PD_RETURNDC, PD_NOPAGENUMS, PD_NOSELECTION, PRINTDLGW, PrintDlgW};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH, FOS_OVERWRITEPROMPT,
    FOS_FORCEFILESYSTEM,
};
use windows::core::{w, PCWSTR};

use crate::d2d;
use crate::destination::{self, NameParts};
use crate::geometry::Rect;
use crate::i18n::{tr, trf};
use crate::model::Shape;
use crate::naming::Kind;
use crate::render::{self, Source};
use crate::session::Finished;
use crate::settings::{self, Settings};
use crate::util::{self, W};
use crate::hud;

/// A finished image: BGRA (premultiplied, opaque), top-down.
#[derive(Clone)]
pub struct Image {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
}

/// Crop the frozen frame to `sel` (pixels) and render `shapes` on top.
pub fn compose(frame: &[u8], fw: u32, fh: u32, sel: Rect, shapes: &[Shape], scale: f64) -> Option<Image> {
    let x0 = sel.x.round().max(0.0) as u32;
    let y0 = sel.y.round().max(0.0) as u32;
    let w = (sel.w.round() as u32).min(fw.saturating_sub(x0)).max(1);
    let h = (sel.h.round() as u32).min(fh.saturating_sub(y0)).max(1);
    let mut crop = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let src = (((y0 + row) * fw + x0) * 4) as usize;
        let dst = (row * w * 4) as usize;
        crop[dst..dst + (w * 4) as usize].copy_from_slice(&frame[src..src + (w * 4) as usize]);
    }
    let dpi = 96.0 * scale;
    if shapes.is_empty() {
        return Some(Image { pixels: crop, width: w, height: h, dpi });
    }
    unsafe {
        let wic = d2d::wic();
        let bmp = wic.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppPBGRA, w * 4, &crop).ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..d2d::premultiplied_props()
        };
        let rt: ID2D1RenderTarget = d2d::factory().CreateWicBitmapRenderTarget(&bmp, &props).ok()?;
        rt.BeginDraw();
        rt.SetTransform(&windows_numerics::Matrix3x2::translation(-(x0 as f32), -(y0 as f32)));
        let src = Source { pixels: frame, width: fw, height: fh, scale };
        render::draw_shapes(&rt, shapes, &src);
        rt.EndDraw(None, None).ok()?;
        drop(rt);
        let mut out = vec![0u8; (w * h * 4) as usize];
        let r = WICRect { X: 0, Y: 0, Width: w as i32, Height: h as i32 };
        bmp.CopyPixels(&r, w * 4, &mut out).ok()?;
        Some(Image { pixels: out, width: w, height: h, dpi })
    }
}

fn stream_bytes(stream: &IStream) -> Option<Vec<u8>> {
    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let len = stat.cbSize as usize;
        let hg = GetHGlobalFromStream(stream).ok()?;
        let p = GlobalLock(hg) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, len).to_vec();
        let _ = GlobalUnlock(hg);
        Some(v)
    }
}

/// Encode as PNG (or JPEG) through WIC.
pub fn encode(img: &Image, jpeg: bool) -> Option<Vec<u8>> {
    unsafe {
        let wic = d2d::wic();
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true).ok()?;
        let fmt = if jpeg { GUID_ContainerFormatJpeg } else { GUID_ContainerFormatPng };
        let enc = wic.CreateEncoder(&fmt, std::ptr::null()).ok()?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut bag: Option<IPropertyBag2> = None;
        enc.CreateNewFrame(&mut frame, &mut bag).ok()?;
        let frame = frame?;
        frame.Initialize(bag.as_ref()).ok()?;
        frame.SetSize(img.width, img.height).ok()?;
        frame.SetResolution(img.dpi, img.dpi).ok()?;
        let mut pf = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut pf).ok()?;
        if pf == GUID_WICPixelFormat32bppBGRA {
            frame.WritePixels(img.height, img.width * 4, &img.pixels).ok()?;
        } else {
            // JPEG wants 24-bit BGR: let WIC convert from a source bitmap.
            let src = wic.CreateBitmapFromMemory(img.width, img.height, &GUID_WICPixelFormat32bppBGRA, img.width * 4, &img.pixels).ok()?;
            frame.WriteSource(&src, std::ptr::null()).ok()?;
        }
        frame.Commit().ok()?;
        enc.Commit().ok()?;
        let _ = WICBitmapCacheOnLoad;
        stream_bytes(&stream)
    }
}

fn global_from(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let hg = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let p = GlobalLock(hg) as *mut u8;
        if p.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(hg);
        Some(hg)
    }
}

/// A bottom-up 32-bit DIBV5 with an alpha mask (what Office, Paint and browsers read).
fn dibv5(img: &Image) -> Vec<u8> {
    let hdr = BITMAPV5HEADER {
        bV5Size: std::mem::size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: img.width as i32,
        bV5Height: img.height as i32,
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: img.width * img.height * 4,
        bV5RedMask: 0x00FF0000,
        bV5GreenMask: 0x0000FF00,
        bV5BlueMask: 0x000000FF,
        bV5AlphaMask: 0xFF000000,
        bV5CSType: 0x7352_4742, // 'sRGB'
        bV5Intent: 4,           // LCS_GM_IMAGES
        ..Default::default()
    };
    let mut out = Vec::with_capacity(std::mem::size_of::<BITMAPV5HEADER>() + img.pixels.len());
    // SAFETY: plain-old-data header.
    out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(&hdr as *const _ as *const u8, std::mem::size_of::<BITMAPV5HEADER>())
    });
    let stride = (img.width * 4) as usize;
    for row in (0..img.height as usize).rev() {
        out.extend_from_slice(&img.pixels[row * stride..row * stride + stride]);
    }
    out
}

pub fn copy_image_to_clipboard(img: &Image) -> bool {
    let png = encode(img, false);
    unsafe {
        if OpenClipboard(Some(util::message_window())).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        if let Some(png) = png.and_then(|b| global_from(&b)) {
            let fmt = RegisterClipboardFormatW(w!("PNG"));
            ok |= SetClipboardData(fmt, Some(HANDLE(png.0))).is_ok();
        }
        if let Some(dib) = global_from(&dibv5(img)) {
            ok |= SetClipboardData(CF_DIBV5.0 as u32, Some(HANDLE(dib.0))).is_ok();
        }
        let _ = CloseClipboard();
        ok
    }
}

fn name_parts(f: &Finished) -> NameParts {
    NameParts { kind: Kind::Screenshot, app: f.front_app.clone(), size: Some((f.image.width, f.image.height)), display: Some(f.display_name.clone()) }
}

/// Copy, plus auto-save when that setting is on.
pub fn copy(f: &Finished) {
    let ok = copy_image_to_clipboard(&f.image);
    let s = settings::get();
    if s.shot_autosave {
        match autosave(f, &s) {
            Ok((path, fell_back)) => {
                let detail = trf("Saved to {folder}", &[("folder", &util::tilde(path.parent().unwrap_or(&path)))]);
                hud::toast(&if fell_back { tr("Copied · cloud folder missing") } else { tr("Copied") }, Some(&detail));
            }
            Err(e) => hud::message(&tr("Copied, but auto-save failed"), Some(&e)),
        }
    } else if ok {
        hud::toast(&tr("Copied"), None);
    } else {
        hud::message(&tr("Couldn't copy"), None);
    }
}

fn autosave(f: &Finished, s: &Settings) -> Result<(PathBuf, bool), String> {
    let resolved = destination::resolve(&s.shot_dir, &Settings::default_shot_dir());
    let (path, counter) = destination::unique_path(&resolved.dir, &s.shot_naming, s.shot_counter, &name_parts(f), "png");
    let data = encode(&f.image, false).ok_or_else(|| tr("Couldn't encode PNG"))?;
    destination::write_atomic(&path, &data)?;
    settings::update(|st| st.shot_counter = counter + 1);
    Ok((path, resolved.fell_back))
}

/// Ask where to save (PNG, or JPEG by type or extension).
pub fn save_with_dialog(f: Finished) {
    let s = settings::get();
    let dir = s.last_save_dir.clone().filter(|d| d.is_dir()).unwrap_or_else(|| {
        let r = destination::resolve(&s.shot_dir, &Settings::default_shot_dir());
        let _ = std::fs::create_dir_all(&r.dir);
        r.dir
    });
    let name = destination::file_name_preview(&s.shot_naming, s.shot_counter, &name_parts(&f), "png");
    let Some(path) = save_dialog(&dir, &name) else { return };
    let jpeg = matches!(path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("jpg" | "jpeg"));
    let result = encode(&f.image, jpeg).ok_or_else(|| tr("Couldn't encode the image")).and_then(|d| destination::write_atomic(&path, &d));
    match result {
        Ok(()) => {
            let parent = path.parent().map(Path::to_path_buf);
            settings::update(|st| {
                st.last_save_dir = parent;
                st.shot_counter += 1;
            });
            hud::toast(&tr("Saved"), Some(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        }
        Err(e) => hud::message(&tr("Couldn't save"), Some(&e)),
    }
}

fn save_dialog(dir: &Path, name: &str) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let (png, jpeg) = (W::new(&tr("PNG image")), W::new(&tr("JPEG image")));
        let specs = [
            COMDLG_FILTERSPEC { pszName: png.p(), pszSpec: w!("*.png") },
            COMDLG_FILTERSPEC { pszName: jpeg.p(), pszSpec: w!("*.jpg;*.jpeg") },
        ];
        let _ = dlg.SetFileTypes(&specs);
        let _ = dlg.SetDefaultExtension(w!("png"));
        let n = W::new(name);
        let _ = dlg.SetFileName(n.p());
        let opts = dlg.GetOptions().unwrap_or_default();
        let _ = dlg.SetOptions(opts | FOS_OVERWRITEPROMPT | FOS_FORCEFILESYSTEM);
        let d = W::new(&dir.to_string_lossy());
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(d.p(), None) {
            let _ = dlg.SetFolder(&item);
        }
        dlg.Show(None).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
        let mut path = PathBuf::from(s?);
        if dlg.GetFileTypeIndex().unwrap_or(1) == 2 && path.extension().is_none_or(|e| e != "jpg" && e != "jpeg") {
            path.set_extension("jpg");
        }
        Some(path)
    }
}

/// Print, scaled to fit the printable area and centred.
pub fn print(f: &Finished) {
    unsafe {
        let mut pd = PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            Flags: PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION,
            nCopies: 1,
            ..Default::default()
        };
        if !PrintDlgW(&mut pd).as_bool() || pd.hDC.is_invalid() {
            return;
        }
        let dc: HDC = pd.hDC;
        let doc = DOCINFOW { cbSize: std::mem::size_of::<DOCINFOW>() as i32, lpszDocName: w!("Photon"), ..Default::default() };
        if StartDocW(dc, &doc) > 0 {
            StartPage(dc);
            let (pw, ph) = (GetDeviceCaps(Some(dc), HORZRES) as f64, GetDeviceCaps(Some(dc), VERTRES) as f64);
            let _ = GetDeviceCaps(Some(dc), LOGPIXELSX);
            let (iw, ih) = (f.image.width as f64, f.image.height as f64);
            let k = (pw / iw).min(ph / ih).min(4.0);
            let (dw, dh) = (iw * k, ih * k);
            let (dx, dy) = ((pw - dw) / 2.0, (ph - dh) / 2.0);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: f.image.width as i32,
                    biHeight: -(f.image.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            StretchDIBits(
                dc,
                dx as i32,
                dy as i32,
                dw as i32,
                dh as i32,
                0,
                0,
                f.image.width as i32,
                f.image.height as i32,
                Some(f.image.pixels.as_ptr() as _),
                &bi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            EndPage(dc);
            EndDoc(dc);
        }
        let _ = DeleteDC(dc);
        let _ = PCWSTR::null();
    }
}
