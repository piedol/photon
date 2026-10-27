//! Turning a selection plus markup into an image, and the copy / save / print actions.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSImageView, NSModalResponseOK, NSPasteboard,
    NSPasteboardTypePNG, NSPrintOperation, NSPrintingPaginationMode, NSSavePanel,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGImageByteOrderInfo, CGContext, CGImage,
    CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSSize, NSString, NSURL};
use objc2_uniform_type_identifiers::UTType;

use crate::annotate::model::Shape;
use crate::annotate::render::{self, Source};
use crate::capture::cg_rect;
use crate::geometry::{view_to_image_px, Rect};
use crate::i18n::{tr, trf};
use crate::output::destination::{self, NameParts};
use crate::output::naming::Kind;
use crate::session::Finished;
use crate::settings::{self, Settings};
use crate::util::{self, ns};
use crate::{hud, ui};

/// Crop `image` to `sel` (view points) and render `shapes` on top at full pixel scale.
pub fn compose(image: &CGImage, sel: Rect, view_h: f64, scale: f64, shapes: &[Shape]) -> Option<CFRetained<CGImage>> {
    let px = view_to_image_px(sel, view_h, scale);
    let px = Rect::new(px.x.round(), px.y.round(), px.w.round().max(1.0), px.h.round().max(1.0));
    let crop = CGImage::with_image_in_rect(Some(image), cg_rect(px))?;
    if shapes.is_empty() {
        return Some(crop);
    }
    let (w, h) = (CGImage::width(Some(&crop)), CGImage::height(Some(&crop)));
    let space = CGImage::color_space(Some(&crop))
        .filter(|s| CGColorSpace::number_of_components(Some(s)) == 3)
        .or_else(|| CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })))?;
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    let ctx = unsafe { CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, 0, Some(&space), info) }?;
    let full = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64));
    CGContext::draw_image(Some(&ctx), full, Some(&crop));
    CGContext::scale_ctm(Some(&ctx), w as f64 / sel.w, h as f64 / sel.h);
    CGContext::translate_ctm(Some(&ctx), -sel.x, -sel.y);
    CGContext::clip_to_rect(Some(&ctx), cg_rect(sel));
    let src = Source { image, view_h, scale };
    render::draw_shapes(&ctx, shapes, &src);
    CGBitmapContextCreateImage(Some(&ctx))
}

fn rep(image: &CGImage, points: (f64, f64)) -> Retained<NSBitmapImageRep> {
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    // Points size makes Retina captures carry 144 dpi, like macOS screenshots.
    rep.setSize(NSSize::new(points.0, points.1));
    rep
}

fn points(f: &Finished) -> (f64, f64) {
    (f.global_rect.w, f.global_rect.h)
}

pub fn png_data(image: &CGImage, pts: (f64, f64)) -> Option<Retained<NSData>> {
    let r = rep(image, pts);
    unsafe { r.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
}

fn jpeg_data(image: &CGImage, pts: (f64, f64)) -> Option<Retained<NSData>> {
    let r = rep(image, pts);
    unsafe { r.representationUsingType_properties(NSBitmapImageFileType::JPEG, &NSDictionary::new()) }
}

pub fn copy_image_to_pasteboard(image: &CGImage, pts: (f64, f64)) -> bool {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let r = rep(image, pts);
    let png = unsafe { r.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) };
    let mut ok = false;
    if let Some(png) = png {
        ok |= unsafe { pb.setData_forType(Some(&png), NSPasteboardTypePNG) };
    }
    // PNG only: every current app reads it, and a TIFF copy would hold ~8 MB per capture.
    ok
}

fn name_parts(f: &Finished) -> NameParts {
    NameParts { kind: Kind::Screenshot, app: f.front_app.clone(), size: Some(f.px), display: Some(f.display_name.clone()) }
}

/// Copy, plus auto-save when that setting is on.
pub fn copy(f: &Finished) {
    let ok = copy_image_to_pasteboard(&f.image, points(f));
    let s = settings::get();
    if s.shot_autosave {
        match autosave(f, &s) {
            Ok((path, fell_back)) => {
                let detail = trf("Saved to {folder}", &[("folder", &util::tilde(path.parent().unwrap_or(&path)))]);
                let title = if fell_back { tr("Copied · cloud folder missing") } else { tr("Copied") };
                hud::toast(&title, Some(&detail));
            }
            Err(e) => hud::toast(&tr("Copied, but auto-save failed"), Some(&e)),
        }
    } else if ok {
        hud::toast(&tr("Copied"), None);
    } else {
        hud::toast(&tr("Couldn't copy"), None);
    }
}

fn autosave(f: &Finished, s: &Settings) -> Result<(PathBuf, bool), String> {
    let resolved = destination::resolve(&s.shot_dir, &Settings::default_shot_dir());
    let (path, counter) = destination::unique_path(&resolved.dir, &s.shot_naming, s.shot_counter, &name_parts(f), "png");
    let data = png_data(&f.image, points(f)).ok_or_else(|| tr("Couldn't encode PNG"))?;
    destination::write_atomic(&path, &data.to_vec())?;
    settings::update(|st| st.shot_counter = counter + 1);
    Ok((path, resolved.fell_back))
}

/// Ask where to save (PNG, or JPEG when the name ends in .jpg/.jpeg).
pub fn save_with_panel(f: Finished) {
    let mtm = MainThreadMarker::new().unwrap();
    let s = settings::get();
    let panel = NSSavePanel::savePanel(mtm);
    let dir = s.last_save_dir.clone().filter(|d| d.is_dir()).unwrap_or_else(|| {
        let r = destination::resolve(&s.shot_dir, &Settings::default_shot_dir());
        let _ = std::fs::create_dir_all(&r.dir);
        r.dir
    });
    let name = destination::file_name_preview(&s.shot_naming, s.shot_counter, &name_parts(&f), "png");
    panel.setNameFieldStringValue(&ns(&name));
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&ns(&dir.to_string_lossy()))));
    let types: Vec<Retained<UTType>> = ["public.png", "public.jpeg"]
        .iter()
        .filter_map(|id| UTType::typeWithIdentifier(&NSString::from_str(id)))
        .collect();
    panel.setAllowedContentTypes(&NSArray::from_retained_slice(&types));
    panel.setAllowsOtherFileTypes(true);
    panel.setCanCreateDirectories(true);
    panel.setExtensionHidden(false);
    panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    ui::app(mtm).activate();
    if panel.runModal() != NSModalResponseOK {
        ui::app(mtm).deactivate();
        return;
    }
    let Some(path) = panel.URL().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())) else { return };
    let jpeg = matches!(path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("jpg" | "jpeg"));
    let data = if jpeg { jpeg_data(&f.image, points(&f)) } else { png_data(&f.image, points(&f)) };
    let result = data.ok_or_else(|| tr("Couldn't encode the image")).and_then(|d| destination::write_atomic(&path, &d.to_vec()));
    ui::app(mtm).deactivate();
    match result {
        Ok(()) => {
            let parent = path.parent().map(Path::to_path_buf);
            settings::update(|st| {
                st.last_save_dir = parent;
                st.shot_counter += 1;
            });
            hud::toast(&tr("Saved"), Some(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        }
        Err(e) => hud::toast(&tr("Couldn't save"), Some(&e)),
    }
}

pub fn print(f: &Finished) {
    let mtm = MainThreadMarker::new().unwrap();
    let (w, h) = points(f);
    let img = NSImage::initWithCGImage_size(NSImage::alloc(), &f.image, NSSize::new(w, h));
    let view = NSImageView::imageViewWithImage(&img, mtm);
    view.setFrame(cg_rect(Rect::new(0.0, 0.0, w, h)));
    let op = NSPrintOperation::printOperationWithView(&view);
    let info = op.printInfo();
    info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
    info.setVerticalPagination(NSPrintingPaginationMode::Fit);
    info.setHorizontallyCentered(true);
    info.setVerticallyCentered(true);
    ui::app(mtm).activate();
    op.runOperation();
    ui::app(mtm).deactivate();
}
