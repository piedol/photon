//! After a recording: the "Recording saved" panel and its actions (Show in
//! Explorer, Copy file, Open, Trim, Make GIF).

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use windows::Foundation::TimeSpan;
use windows::Media::MediaProperties::MediaEncodingProfile;
use windows::Media::Transcoding::MediaTranscoder;
use windows::Storage::{CreationCollisionOption, StorageFile, StorageFolder};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Imaging::{
    GUID_ContainerFormatGif, GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat8bppIndexed, IWICBitmapFrameEncode,
    WICBitmapDitherTypeErrorDiffusion, WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
};
use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, CAUB};
use windows::Win32::System::Com::{CoTaskMemAlloc, IStream};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_HDROP;
use windows::Win32::System::Variant::{VT_UI1, VT_UI2, VT_VECTOR};
use windows::Win32::UI::Shell::DROPFILES;
use windows::core::HSTRING;

use super::mf::{self, RgbReader};
use crate::commands::Command;
use crate::d2d;
use crate::geometry::Rect;
use crate::hud::{self, Button, Content};
use crate::i18n::tr;
use crate::toolbar::Icon;
use crate::util;

mod trim;

pub use trim::is_open as trimming;

struct Finished {
    path: PathBuf,
    hud: u64,
}

thread_local! {
    static LAST: RefCell<Option<Finished>> = const { RefCell::new(None) };
}

/// "Recording saved" with Show / Copy / Open / Trim / GIF, bottom-right.
pub fn finished_hud(path: PathBuf, fell_back: bool) {
    dismiss();
    let is_video = path.extension().is_some_and(|e| e == "mp4");
    let c = hud::cursor();
    let (s, work, _) = hud::monitor_at(c);
    let btn = 34.0 * s;
    let mut actions: Vec<(Icon, String, Command)> = vec![
        (Icon::Folder, tr("Show in Explorer"), Command::Reveal),
        (Icon::Copy, tr("Copy file"), Command::CopyFile),
        (Icon::Play, tr("Open"), Command::OpenFile),
    ];
    if is_video {
        actions.push((Icon::Trim, tr("Trim"), Command::Trim));
        actions.push((Icon::Gif, tr("Make GIF"), Command::ExportGif));
    }
    let title = if fell_back {
        tr("Saved locally · cloud folder missing")
    } else if is_video {
        tr("Recording saved")
    } else {
        tr("GIF saved")
    };
    // Wide enough for the title, which is longer in some languages, and the close button.
    let title_w = d2d::measure(&d2d::layout(&title, 13.0 * s, d2d::WEIGHT_SEMIBOLD)).0;
    let (w, h) = ((330.0 * s).max(title_w + 66.0 * s), 96.0 * s);
    let r = Rect::new(work.max_x() - w - 16.0 * s, work.max_y() - h - 16.0 * s, w, h);
    let mut buttons: Vec<Button> = actions
        .iter()
        .enumerate()
        .map(|(i, (icon, tip, cmd))| Button {
            rect: Rect::new(10.0 * s + i as f64 * (btn + 2.0 * s), h - btn - 8.0 * s, btn, btn),
            cmd: *cmd,
            icon: Some(*icon),
            label: None,
            tooltip: tip.clone(),
        })
        .collect();
    buttons.push(Button { rect: Rect::new(w - 34.0 * s, 8.0 * s, 26.0 * s, 26.0 * s), cmd: Command::DismissHud, icon: Some(Icon::Close), label: None, tooltip: tr("Dismiss") });
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let detail = format!("{name} · {}", util::tilde(path.parent().unwrap_or(&path)));
    let id = hud::show(r, s, Content::Saved { title, detail }, buttons, false, false);
    hud::dismiss_after(id, 10.0);
    LAST.with(|l| *l.borrow_mut() = Some(Finished { path, hud: id }));
}

fn dismiss() {
    if let Some(f) = LAST.with(|l| l.borrow_mut().take()) {
        hud::fade_out(f.hud);
    }
}

fn last_path() -> Option<PathBuf> {
    LAST.with(|l| l.borrow().as_ref().map(|f| f.path.clone()))
}

/// Developer self-test helpers.
pub fn close_trim_for_test() {
    trim::close_for_test();
}

pub fn trim_last_video_for_test() {
    let dir = crate::settings::get().rec_dir;
    let newest = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "mp4"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path())
    });
    if let Some(p) = newest {
        trim::open(p);
    }
}

pub fn gif_last_video_for_test() {
    let dir = crate::settings::get().rec_dir;
    let newest = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "mp4"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path())
    });
    if let Some(p) = newest {
        make_gif(p);
    }
}

pub fn command_for_test(cmd: Command) {
    command(cmd);
}

pub fn command(cmd: Command) {
    let Some(path) = last_path() else { return };
    match cmd {
        Command::Reveal => {
            let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
            dismiss();
        }
        Command::CopyFile => {
            if copy_file(&path) {
                hud::toast(&tr("File copied"), Some(&tr("Paste it into Discord, Slack or Explorer")));
            }
            dismiss();
        }
        Command::OpenFile => {
            crate::app::shell_open(&path.to_string_lossy());
            dismiss();
        }
        Command::Trim => {
            dismiss();
            trim::open(path);
        }
        Command::ExportGif => {
            dismiss();
            make_gif(path);
        }
        Command::DismissHud => dismiss(),
        _ => {}
    }
}

/// Put the file on the clipboard (CF_HDROP), so it pastes as a file.
fn copy_file(path: &Path) -> bool {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0, 0]).collect();
    let header = std::mem::size_of::<DROPFILES>();
    let total = header + wide.len() * 2;
    unsafe {
        let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total) else { return false };
        let p = GlobalLock(hg) as *mut u8;
        if p.is_null() {
            return false;
        }
        let df = DROPFILES { pFiles: header as u32, fWide: true.into(), ..Default::default() };
        std::ptr::copy_nonoverlapping(&df as *const _ as *const u8, p, header);
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(header), wide.len() * 2);
        let _ = GlobalUnlock(hg);
        if OpenClipboard(Some(util::message_window())).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hg.0))).is_ok();
        let _ = CloseClipboard();
        let _ = HGLOBAL::default();
        ok
    }
}

/// Cut `[start, end)` seconds out of `path` without re-encoding, replacing it.
pub fn apply_trim(path: PathBuf, start: f64, end: f64) {
    hud::toast(&tr("Trimming…"), None);
    let busy = util::Busy::start();
    std::thread::spawn(move || {
        let result = trim_file(&path, start, end);
        util::main_async(move || {
            let _busy = busy;
            match result {
                Ok(()) => finished_hud(path, false),
                Err(e) => hud::message(&tr("Couldn't trim the recording"), Some(&e)),
            }
        });
    });
}

fn trim_file(path: &Path, start: f64, end: f64) -> Result<(), String> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let e = |x: windows::core::Error| x.message().to_string();
    let dir = path.parent().ok_or_else(|| tr("bad path"))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp_name = format!("{stem}.trimming.mp4");
    let src = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
        .map_err(e)?
        .join()
        .map_err(e)?;
    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(dir.as_os_str())).map_err(e)?.join().map_err(e)?;
    let dst = folder.CreateFileAsync(&HSTRING::from(tmp_name.as_str()), CreationCollisionOption::ReplaceExisting).map_err(e)?.join().map_err(e)?;
    let profile = MediaEncodingProfile::CreateFromFileAsync(&src).map_err(e)?.join().map_err(e)?;
    let t = MediaTranscoder::new().map_err(e)?;
    t.SetAlwaysReencode(false).map_err(e)?;
    t.SetTrimStartTime(TimeSpan { Duration: (start * 10_000_000.0) as i64 }).map_err(e)?;
    let total = RgbReader::file(path).map(|r| r.duration()).unwrap_or(0.0);
    if end < total - 0.05 {
        // Despite its documentation, TrimStopTime behaves as the absolute end position.
        t.SetTrimStopTime(TimeSpan { Duration: (end * 10_000_000.0) as i64 }).map_err(e)?;
    }
    let prep = t.PrepareFileTranscodeAsync(&src, &dst, &profile).map_err(e)?.join().map_err(e)?;
    if !prep.CanTranscode().map_err(e)? {
        return Err(tr("Windows can't trim this file."));
    }
    prep.TranscodeAsync().map_err(e)?.join().map_err(e)?;
    drop((src, dst));
    let tmp = dir.join(&tmp_name);
    std::fs::rename(&tmp, path).map_err(|x| x.to_string())
}

// ── GIF ────────────────────────────────────────────────────────────────────

const GIF_FPS: f64 = 12.0;
const GIF_MAX_SECONDS: f64 = 60.0;
const GIF_MAX_WIDTH: u32 = 960;

fn make_gif(video: PathBuf) {
    hud::toast(&tr("Making GIF…"), None);
    let out = video.with_extension("gif");
    let busy = util::Busy::start();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let result = write_gif(&video, &out).map(|()| out);
        util::main_async(move || {
            let _busy = busy;
            match result {
                Ok(path) => finished_hud(path, false),
                Err(e) => hud::message(&tr("Couldn't make the GIF"), Some(&e)),
            }
        });
    });
}

fn pv_u16(v: u16) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 { vt: VT_UI2, wReserved1: 0, wReserved2: 0, wReserved3: 0, Anonymous: PROPVARIANT_0_0_0 { uiVal: v } }),
        },
    }
}

fn pv_bytes(b: &[u8]) -> PROPVARIANT {
    unsafe {
        let p = CoTaskMemAlloc(b.len()) as *mut u8;
        std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
        PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_UI1 | VT_VECTOR,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 { caub: CAUB { cElems: b.len() as u32, pElems: p } },
                }),
            },
        }
    }
}

fn write_gif(video: &Path, out: &Path) -> Result<(), String> {
    let reader = RgbReader::file(video).ok_or_else(|| tr("Couldn't open the recording."))?;
    let wic = d2d::wic();
    let e = |x: windows::core::Error| x.message().to_string();
    unsafe {
        let stream: IStream = {
            let s = wic.CreateStream().map_err(e)?;
            let p = util::W::new(&out.to_string_lossy());
            s.InitializeFromFilename(p.p(), 0x40000000 /* GENERIC_WRITE */).map_err(e)?;
            windows_core::Interface::cast(&s).map_err(e)?
        };
        let enc = wic.CreateEncoder(&GUID_ContainerFormatGif, std::ptr::null()).map_err(e)?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache).map_err(e)?;
        // Loop forever (NETSCAPE2.0 application extension).
        if let Ok(meta) = enc.GetMetadataQueryWriter() {
            let _ = meta.SetMetadataByName(windows::core::w!("/appext/application"), &pv_bytes(b"NETSCAPE2.0"));
            let _ = meta.SetMetadataByName(windows::core::w!("/appext/data"), &pv_bytes(&[3, 1, 0, 0, 0]));
        }
        let mut next_t = 0.0f64;
        let mut frames = 0u32;
        let mut carry = 0.0f64;
        while let Some(f) = reader.next() {
            let t = f.time as f64 / 10_000_000.0;
            if t > GIF_MAX_SECONDS {
                break;
            }
            if t + 1e-6 < next_t {
                continue;
            }
            next_t += 1.0 / GIF_FPS;
            let (px, w, h) = mf::fit(&f, GIF_MAX_WIDTH, GIF_MAX_WIDTH);
            let src = wic.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, w * 4, &px).map_err(e)?;
            let palette = wic.CreatePalette().map_err(e)?;
            palette.InitializeFromBitmap(&src, 256, false).map_err(e)?;
            let conv = wic.CreateFormatConverter().map_err(e)?;
            conv.Initialize(&src, &GUID_WICPixelFormat8bppIndexed, WICBitmapDitherTypeErrorDiffusion, &palette, 0.0, WICBitmapPaletteTypeCustom)
                .map_err(e)?;
            let mut frame: Option<IWICBitmapFrameEncode> = None;
            let mut bag: Option<IPropertyBag2> = None;
            enc.CreateNewFrame(&mut frame, &mut bag).map_err(e)?;
            let frame = frame.ok_or_else(|| tr("frame"))?;
            frame.Initialize(bag.as_ref()).map_err(e)?;
            frame.SetSize(w, h).map_err(e)?;
            let mut pf = GUID_WICPixelFormat8bppIndexed;
            frame.SetPixelFormat(&mut pf).map_err(e)?;
            frame.SetPalette(&palette).map_err(e)?;
            // Delay in 1/100 s; carry the remainder so 12 fps averages correctly.
            carry += 100.0 / GIF_FPS;
            let delay = carry.floor();
            carry -= delay;
            if let Ok(meta) = frame.GetMetadataQueryWriter() {
                let _ = meta.SetMetadataByName(windows::core::w!("/grctlext/Delay"), &pv_u16(delay as u16));
            }
            frame.WriteSource(&conv, std::ptr::null()).map_err(e)?;
            frame.Commit().map_err(e)?;
            frames += 1;
        }
        if frames == 0 {
            return Err(tr("The recording has no frames."));
        }
        enc.Commit().map_err(e)?;
    }
    Ok(())
}
