//! Screen Recording, Microphone and Camera permission checks.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSWorkspace};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio, AVMediaTypeVideo};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use objc2_foundation::{MainThreadMarker, NSURL};

use crate::util::ns;
use crate::ui;

pub fn screen_ok() -> bool {
    CGPreflightScreenCaptureAccess()
}

/// Trigger the system prompt; if it was denied before, explain and open Settings.
pub fn ask_screen() {
    if CGRequestScreenCaptureAccess() {
        return;
    }
    let mtm = MainThreadMarker::new().unwrap();
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Informational);
    alert.setMessageText(&ns("Photon needs Screen Recording access"));
    alert.setInformativeText(&ns(
        "Turn on Photon in System Settings › Privacy & Security › Screen & System Audio Recording, then press the shortcut again.",
    ));
    alert.addButtonWithTitle(&ns("Open System Settings"));
    alert.addButtonWithTitle(&ns("Not Now"));
    ui::app(mtm).activate();
    if alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn {
        open_settings("Privacy_ScreenCapture");
    }
}

fn open_settings(anchor: &str) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    if let Some(u) = NSURL::URLWithString(&ns(&url)) {
        NSWorkspace::sharedWorkspace().openURL(&u);
    }
}

fn ask(media_video: bool, anchor: &'static str) {
    let ty = unsafe { if media_video { AVMediaTypeVideo } else { AVMediaTypeAudio } };
    let Some(ty) = ty else { return };
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(ty) };
    if status == AVAuthorizationStatus::NotDetermined {
        let block = RcBlock::new(|_granted: Bool| {});
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(ty, &block) };
    } else if status == AVAuthorizationStatus::Denied || status == AVAuthorizationStatus::Restricted {
        crate::hud::message(
            if media_video { "Camera access is off for Photon" } else { "Microphone access is off for Photon" },
            Some("Turn it on in System Settings › Privacy & Security."),
        );
        open_settings(anchor);
    }
}

pub fn ask_microphone() {
    ask(false, "Privacy_Microphone");
}

pub fn ask_camera() {
    ask(true, "Privacy_Camera");
}

pub fn authorized(video: bool) -> bool {
    let ty = unsafe { if video { AVMediaTypeVideo } else { AVMediaTypeAudio } };
    ty.is_some_and(|ty| unsafe { AVCaptureDevice::authorizationStatusForMediaType(ty) } == AVAuthorizationStatus::Authorized)
}
