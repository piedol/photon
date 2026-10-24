//! Webcam bubble: a round, draggable camera preview that is included in the recording.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSFloatingWindowLevel, NSPanel, NSView};
use objc2_av_foundation::{
    AVCaptureDevice, AVCaptureDeviceInput, AVCaptureSession, AVCaptureSessionPresetHigh,
    AVCaptureVideoPreviewLayer, AVLayerVideoGravityResizeAspectFill, AVMediaTypeVideo,
};
use objc2_quartz_core::CALayer;

use crate::capture::cg_rect;
use crate::geometry::Rect;
use crate::overlay::chrome::rgba;
use crate::util::{self, MainBound};
use crate::hud;

pub struct Bubble {
    panel: Retained<NSPanel>,
    session: Retained<AVCaptureSession>,
}

const SIZE: f64 = 168.0;
const INSET: f64 = 24.0;

impl Bubble {
    pub fn show(mtm: MainThreadMarker, region: Rect, device_id: Option<&str>) -> Option<Bubble> {
        let device = unsafe {
            device_id
                .and_then(|id| AVCaptureDevice::deviceWithUniqueID(&crate::util::ns(id)))
                .or_else(|| AVMediaTypeVideo.and_then(|t| AVCaptureDevice::defaultDeviceWithMediaType(t)))
        }?;
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }.ok()?;
        let session = unsafe { AVCaptureSession::new() };
        unsafe {
            session.setSessionPreset(AVCaptureSessionPresetHigh);
            if !session.canAddInput(&input) {
                return None;
            }
            session.addInput(&input);
        }

        let size = SIZE.min(region.w * 0.4).min(region.h * 0.4).max(72.0);
        let frame = Rect::new(region.max_x() - size - INSET, region.y + INSET, size, size);
        let panel = hud::panel(mtm, frame, NSFloatingWindowLevel);
        panel.setMovableByWindowBackground(true);
        panel.setHasShadow(true);
        let view = NSView::initWithFrame(NSView::alloc(mtm), cg_rect(Rect::new(0.0, 0.0, size, size)));
        let root = CALayer::new();
        root.setCornerRadius(size / 2.0);
        root.setMasksToBounds(true);
        root.setBorderWidth(2.5);
        root.setBorderColor(Some(&rgba(1.0, 1.0, 1.0, 0.95)));
        root.setBackgroundColor(Some(&rgba(0.1, 0.1, 0.1, 1.0)));
        let preview = unsafe { AVCaptureVideoPreviewLayer::layerWithSession(&session) };
        unsafe {
            if let Some(g) = AVLayerVideoGravityResizeAspectFill {
                preview.setVideoGravity(g);
            }
            if let Some(conn) = preview.connection() {
                conn.setAutomaticallyAdjustsVideoMirroring(false);
                conn.setVideoMirrored(true);
            }
        }
        preview.setFrame(cg_rect(Rect::new(0.0, 0.0, size, size)));
        root.addSublayer(&preview);
        view.setLayer(Some(&root));
        view.setWantsLayer(true);
        view.setToolTip(Some(&crate::util::ns("Drag to move the camera bubble")));
        panel.setContentView(Some(&view));
        panel.orderFrontRegardless();

        // startRunning blocks for a moment; keep it off the main thread.
        let s = MainBound(session.clone());
        dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::QualityOfService(
            dispatch2::DispatchQoS::UserInitiated,
        ))
        .exec_async(move || {
            let whole = s;
            unsafe { whole.0.startRunning() };
            util::log("webcam running");
        });
        Some(Bubble { panel, session })
    }

    pub fn window_number(&self) -> u32 {
        self.panel.windowNumber() as u32
    }
}

impl Drop for Bubble {
    fn drop(&mut self) {
        let s = MainBound(self.session.clone());
        dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::QualityOfService(
            dispatch2::DispatchQoS::Utility,
        ))
        .exec_async(move || {
            let whole = s;
            unsafe { whole.0.stopRunning() };
        });
        self.panel.orderOut(None);
        self.panel.close();
    }
}
