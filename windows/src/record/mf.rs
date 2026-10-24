//! Media Foundation helpers: read frames as 32-bit RGB from a camera or a video
//! file (used by the webcam bubble, the trim window and GIF export).

use std::path::Path;
use std::sync::Once;

use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFActivate, IMFAttributes, IMFMediaSource, IMFSample, IMFSourceReader, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromMediaSource, MFCreateSourceReaderFromURL, MFEnumDeviceSources, MFMediaType_Video,
    MFSTARTUP_FULL, MFStartup, MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, PWSTR};

use crate::util::W;

static START: Once = Once::new();

pub fn startup() {
    START.call_once(|| unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    });
}

fn attrs(n: u32) -> Option<IMFAttributes> {
    let mut a: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut a, n).ok()? };
    a
}

fn string_attr(a: &IMFActivate, key: &GUID) -> Option<String> {
    unsafe {
        let mut p = PWSTR::null();
        let mut len = 0u32;
        a.GetAllocatedString(key, &mut p, &mut len).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

fn camera_activates() -> Vec<IMFActivate> {
    startup();
    let mut out = Vec::new();
    unsafe {
        let Some(a) = attrs(1) else { return out };
        if a.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID).is_err() {
            return out;
        }
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        if MFEnumDeviceSources(&a, &mut list, &mut count).is_err() || list.is_null() {
            return out;
        }
        for i in 0..count as usize {
            if let Some(act) = (*list.add(i)).take() {
                out.push(act);
            }
        }
        CoTaskMemFree(Some(list as _));
    }
    out
}

/// Cameras for the Settings picker: (symbolic link id, friendly name).
pub fn cameras() -> Vec<(String, String)> {
    camera_activates()
        .iter()
        .filter_map(|a| Some((string_attr(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?, string_attr(a, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)?)))
        .collect()
}

pub struct RgbReader {
    reader: IMFSourceReader,
    pub width: u32,
    pub height: u32,
    bottom_up: bool,
}

/// One decoded frame: BGRA top-down, alpha forced opaque.
pub struct RgbFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 100 ns ticks.
    pub time: i64,
}

impl RgbReader {
    fn configure(reader: IMFSourceReader) -> Option<RgbReader> {
        unsafe {
            let mt = MFCreateMediaType().ok()?;
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            reader.SetCurrentMediaType(stream, None, &mt).ok()?;
            let cur = reader.GetCurrentMediaType(stream).ok()?;
            let size = cur.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
            let (width, height) = ((size >> 32) as u32, (size & 0xffff_ffff) as u32);
            // RGB in Media Foundation is bottom-up unless the stride says otherwise.
            let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or(-1);
            Some(RgbReader { reader, width, height, bottom_up: stride < 0 })
        }
    }

    pub fn camera(id: Option<&str>) -> Option<RgbReader> {
        let acts = camera_activates();
        let act = id
            .and_then(|id| acts.iter().find(|a| string_attr(a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).as_deref() == Some(id)))
            .or_else(|| acts.first())?;
        unsafe {
            let source: IMFMediaSource = act.ActivateObject().ok()?;
            let a = attrs(1)?;
            a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
            let reader = MFCreateSourceReaderFromMediaSource(&source, &a).ok()?;
            Self::configure(reader)
        }
    }

    pub fn file(path: &Path) -> Option<RgbReader> {
        startup();
        unsafe {
            let a = attrs(1)?;
            a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).ok()?;
            let url = W::new(&path.to_string_lossy());
            let reader = MFCreateSourceReaderFromURL(url.p(), &a).ok()?;
            Self::configure(reader)
        }
    }

    /// Duration in seconds (files only).
    pub fn duration(&self) -> f64 {
        unsafe {
            self.reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|v: PROPVARIANT| u64::try_from(&v).ok())
                .map(|t| t as f64 / 10_000_000.0)
                .unwrap_or(0.0)
        }
    }

    pub fn seek(&self, seconds: f64) {
        let v = PROPVARIANT::from((seconds * 10_000_000.0) as i64);
        unsafe {
            let _ = self.reader.SetCurrentPosition(&GUID::zeroed(), &v);
        }
    }

    /// The next frame, or None at the end / on error.
    pub fn next(&self) -> Option<RgbFrame> {
        unsafe {
            loop {
                let mut flags = 0u32;
                let mut time = 0i64;
                let mut sample: Option<IMFSample> = None;
                self.reader
                    .ReadSample(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))
                    .ok()?;
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return None;
                }
                let Some(sample) = sample else { continue };
                let buf = match sample.GetBufferCount() {
                    Ok(1) => sample.GetBufferByIndex(0).ok()?,
                    _ => sample.ConvertToContiguousBuffer().ok()?,
                };
                let (w, h) = (self.width as usize, self.height as usize);
                let stride = w * 4;
                let mut pixels = vec![0u8; stride * h];
                // Decoders pad rows; the 2D buffer reports the real pitch (negative
                // when bottom-up) and a pointer to the top row.
                if let Ok(b2) = windows_core::Interface::cast::<IMF2DBuffer>(&buf) {
                    let mut scan0 = std::ptr::null_mut();
                    let mut pitch = 0i32;
                    b2.Lock2D(&mut scan0, &mut pitch).ok()?;
                    for row in 0..h {
                        let src = scan0.offset(row as isize * pitch as isize);
                        std::ptr::copy_nonoverlapping(src, pixels.as_mut_ptr().add(row * stride), stride);
                    }
                    let _ = b2.Unlock2D();
                } else {
                    let mut ptr = std::ptr::null_mut();
                    let mut len = 0u32;
                    buf.Lock(&mut ptr, None, Some(&mut len)).ok()?;
                    let src = std::slice::from_raw_parts(ptr, len as usize);
                    let pitch = pitch_for(len as usize, w, h);
                    for row in 0..h {
                        let s = if self.bottom_up { (h - 1 - row) * pitch } else { row * pitch };
                        if s + stride <= src.len() {
                            pixels[row * stride..row * stride + stride].copy_from_slice(&src[s..s + stride]);
                        }
                    }
                    let _ = buf.Unlock();
                }
                for a in pixels.iter_mut().skip(3).step_by(4) {
                    *a = 255;
                }
                return Some(RgbFrame { pixels, width: self.width, height: self.height, time });
            }
        }
    }
}

/// Row pitch of a plain decoder buffer. Decoders pad frames to block sizes
/// (e.g. 1126×690 arrives as 1136×704), so find the alignment that divides
/// the buffer evenly into at least `h` rows.
fn pitch_for(len: usize, w: usize, h: usize) -> usize {
    for align in [1usize, 2, 16, 32, 64, 128] {
        let aw = w.div_ceil(align) * align;
        let pitch = aw * 4;
        if len.is_multiple_of(pitch) && len / pitch >= h {
            return pitch;
        }
    }
    (len / h.max(1)).max(w * 4)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pitch_detection() {
        assert_eq!(super::pitch_for(1136 * 704 * 4, 1126, 690), 1136 * 4);
        assert_eq!(super::pitch_for(1920 * 1080 * 4, 1920, 1080), 1920 * 4);
        assert_eq!(super::pitch_for(1920 * 1088 * 4, 1920, 1080), 1920 * 4);
    }
}

/// Nearest-neighbour scale of a BGRA frame into `w × h`, centre-cropped to keep aspect.
pub fn scale_cover(src: &RgbFrame, w: u32, h: u32, mirror: bool) -> Vec<u8> {
    let (sw, sh) = (src.width as f64, src.height as f64);
    let k = (w as f64 / sw).max(h as f64 / sh);
    let (cw, ch) = (w as f64 / k, h as f64 / k);
    let (ox, oy) = ((sw - cw) / 2.0, (sh - ch) / 2.0);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = (oy + (y as f64 + 0.5) / k) as usize;
        for x in 0..w {
            let xx = if mirror { w - 1 - x } else { x };
            let sx = (ox + (xx as f64 + 0.5) / k) as usize;
            let si = (sy.min(src.height as usize - 1) * src.width as usize + sx.min(src.width as usize - 1)) * 4;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src.pixels[si..si + 4]);
        }
    }
    out
}

/// Fit inside `max × max`, keeping aspect (for previews and GIFs).
pub fn fit(src: &RgbFrame, max_w: u32, max_h: u32) -> (Vec<u8>, u32, u32) {
    let k = (max_w as f64 / src.width as f64).min(max_h as f64 / src.height as f64).min(1.0);
    let w = ((src.width as f64 * k).round() as u32).max(1);
    let h = ((src.height as f64 * k).round() as u32).max(1);
    // Box-average when shrinking a lot, nearest otherwise.
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (sx, sy) = (src.width as f64 / w as f64, src.height as f64 / h as f64);
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = ((x as f64 * sx) as u32, (y as f64 * sy) as u32);
            let (x1, y1) = (((x + 1) as f64 * sx) as u32, ((y + 1) as f64 * sy) as u32);
            let (mut b, mut g, mut r, mut n) = (0u32, 0u32, 0u32, 0u32);
            let step = ((x1 - x0).max(1) / 3).max(1);
            let mut yy = y0;
            while yy < y1.max(y0 + 1).min(src.height) {
                let mut xx = x0;
                while xx < x1.max(x0 + 1).min(src.width) {
                    let i = ((yy * src.width + xx) * 4) as usize;
                    b += src.pixels[i] as u32;
                    g += src.pixels[i + 1] as u32;
                    r += src.pixels[i + 2] as u32;
                    n += 1;
                    xx += step;
                }
                yy += step;
            }
            let n = n.max(1);
            let o = ((y * w + x) * 4) as usize;
            out[o] = (b / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (r / n) as u8;
            out[o + 3] = 255;
        }
    }
    (out, w, h)
}
