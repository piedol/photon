# Permissions

Photon has no user accounts or roles. Its access model is macOS privacy permissions (TCC), the entitlements in its signature, and a temporary system-wide Esc shortcut.

## macOS privacy permissions

| Permission | Needed for | When it's asked | If denied |
|---|---|---|---|
| Screen & System Audio Recording | Every screenshot and recording, and system audio | At launch, through `CGRequestScreenCaptureAccess` | A dialog explains and offers **Open System Settings**. Captures fail until it's granted. |
| Microphone | The record bar's mic toggle | The first time you turn the mic on | The mic is skipped for that recording, and a HUD opens Privacy › Microphone |
| Camera | The webcam bubble | The first time you turn the webcam on | The bubble is skipped, and a HUD opens Privacy › Camera |

**Not needed:**
- **Accessibility:** global shortcuts use Carbon hotkeys, and click highlights come from ScreenCaptureKit, not an event tap.
- **Input Monitoring.**
- **Full Disk Access.**

macOS may also show its periodic "… is requesting to bypass the system private window picker" prompt for apps that use ScreenCaptureKit directly. Choose **Allow** to keep Photon working without the picker.

## Entitlements and signing

`resources/Photon.entitlements` (hardened runtime):

| Entitlement | Why |
|---|---|
| `com.apple.security.device.audio-input` | Microphone capture |
| `com.apple.security.device.camera` | Webcam bubble |

`Info.plist` carries the purpose strings `NSMicrophoneUsageDescription` and `NSCameraUsageDescription`.

Permissions are tied to the code signature. A stable **Developer ID** signature (`scripts/bundle.sh` uses it when it's in your keychain) keeps the permissions across rebuilds. An ad-hoc signature changes with every build, so macOS asks again. See [Deployment](deployment.md).

**Running the binary from a terminal** (for example during development) makes the terminal app the "responsible process". macOS then asks for permission *for the terminal*, not for Photon.

## The temporary Esc shortcut

While a selection overlay or a recording countdown is on screen, Photon registers **Esc** as a global hotkey. Esc then always cancels, even if macOS didn't give Photon the keyboard. It's unregistered as soon as the overlay closes or the countdown ends. Outside those moments, Esc belongs to whatever app you're using.

## Launch at Login

This uses `SMAppService.mainApp`. macOS may show a "Login item added" notice. You can review it in System Settings › General › Login Items.
