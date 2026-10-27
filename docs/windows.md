# Photon for Windows

Photon for Windows is a native Windows edition with the same features as the Mac app. It is a separate program in [`windows/`](../windows) with its own build, so the Mac app is never affected by Windows changes.

## Requirements

- Windows 10 (version 1903 or later) or Windows 11, 64-bit (x64).
- Windows 11 is recommended for recording. On Windows 10, Windows draws a thin yellow border around whatever is being recorded. The border is not in the video.

## Install

1. Download `Photon-windows-x64.zip` from the repo's [Releases](https://github.com/piedol/photon/releases).
2. Unzip `Photon.exe` anywhere you like, for example `%LOCALAPPDATA%\Programs\Photon`.
3. Run it. The build is not code-signed, so Windows SmartScreen may warn the first time: click **More info › Run anyway**.
4. Photon appears in the notification area (system tray). Press **Ctrl+Shift+9**.
5. To start Photon with Windows, right-click the tray icon and turn on **Launch at Login**.

There are no installers, services or drivers. Photon is one 0.9 MB program.

## How it differs from the Mac app

Everything in [Usage](usage.md) applies, with these Windows equivalents:

| Mac | Windows |
|---|---|
| **⇧⌘9** screenshot | **Ctrl+Shift+9** |
| **⇧⌘0** record | **Ctrl+Shift+0** |
| **⌘C · ⌘S · ⌘P · ⌘Z** | **Ctrl+C · Ctrl+S · Ctrl+P · Ctrl+Z** |
| Menu-bar icon and timer | Tray icon: turns red while recording. Left-click stops, right-click opens the menu. |
| Show in Finder | Show in Explorer |
| QuickTime-style trimmer | Photon's own trim window: filmstrip, drag the yellow handles, **Enter** to trim. Still lossless. |
| macOS privacy prompts | No prompts. For the mic or webcam, Windows must allow desktop apps under **Settings › Privacy**. |
| System accent colour | Windows accent colour |
| `open photon://capture` | `start photon://capture` (also `record`, `stop`, `settings`) |

Both shortcuts can be changed in **Settings › General**, as on the Mac.

## Where things are stored

| What | Where |
|---|---|
| Screenshots (when auto-save is on) | `Pictures\Photon` by default, following OneDrive folder backup if it's on |
| Recordings | `Videos\Photon` by default |
| Settings | `%APPDATA%\Photon\settings.ini` (same keys as the Mac preferences) |
| Launch at Login | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `Photon` |
| `photon://` links | `HKCU\Software\Classes\photon`, written at each launch for the current user only |
| Log (only with `PHOTON_LOG`) | `%LOCALAPPDATA%\Photon\photon.log` |

Cloud destinations work the same way as on the Mac. Photon detects OneDrive (personal and work), Google Drive, Dropbox, Box and iCloud Drive folders, writes into them, and lets the provider's own app upload. Photon has no network code.

## Privacy

- Photon's own panels (the recording pill, countdown, toasts and the saved panel) are hidden from screenshots and recordings.
- The webcam bubble and click highlights are meant to be seen, so they are recorded.
- Clicking a window records only that window. Other windows never appear, even if they cover it.

## Performance

Measured on Windows 10 at 2560×1440:
- About 40 ms from the shortcut to the frozen overlay.
- About 5 MB of private memory at launch. After captures it settles around 30 to 45 MB committed, while the resident set is trimmed to under 1 MB when idle.
- Recording uses the GPU's hardware H.264 or HEVC encoder at 60 fps and writes straight to disk.

## Building from source

You need Windows with:
- Rust using the MSVC toolchain (`x86_64-pc-windows-msvc`).
- Visual Studio Build Tools with the C++ workload and a Windows 10/11 SDK.

```powershell
cd windows
cargo build --release
```

For a release build, keep your user folder's path out of the binary first:

```powershell
$env:RUSTFLAGS = "--remap-path-prefix=$env:USERPROFILE\.cargo=C:\cargo --remap-path-prefix=$((Get-Location).Path)=C:\photon"
```

The program is `windows\target\release\Photon.exe`. Run the unit tests with `cargo test --release`.

On a Mac you can type-check the Windows code without linking it:

```bash
rustup target add x86_64-pc-windows-msvc
cd windows && cargo check --target x86_64-pc-windows-msvc
```

## How it's built

The Windows edition reuses the Mac app's platform-free code by including those files unchanged:
- `geometry.rs`
- `overlay/selection.rs` (selection, handles, nudging)
- `annotate/model.rs` (the markup shapes)
- `output/naming.rs` (file-name templates)
- `output/destination.rs` (folders and fallbacks)
- `commands.rs`

Everything that touches the OS is Windows-native:

| Module | Job |
|---|---|
| `app.rs` | Single instance, tray icon and menu, `photon://` links, Launch at Login, the message loop |
| `hotkey.rs`, `keys.rs` | Global shortcuts (`RegisterHotKey`), plus a temporary Esc shortcut while a selection is open |
| `capture.rs` | Monitor list, frozen-frame grab, window list with friendly app names |
| `overlay.rs`, `session.rs`, `toolbar.rs`, `render.rs` | The overlay windows, selection and markup, drawn with software Direct2D to keep memory low |
| `export.rs` | Compose the final image, PNG/JPEG encoding, clipboard, save dialog, printing |
| `hud.rs`, `pin.rs` | Toasts, countdown, recording pill, saved panel, pinned screenshots |
| `record/` | Windows.Graphics.Capture recording, Media Foundation encoding, WASAPI system audio and mic, webcam bubble, click highlights, trim and GIF |
| `settings.rs`, `settings_window.rs` | The INI settings file and the Settings window |
| `cloud.rs` | Cloud-folder detection and known folders |
| `selftest.rs` | Scripted sessions for development (only with `PHOTON_DEV`) |

## Self-tests

Start Photon with developer mode and logging on, then trigger a scripted session:

```powershell
$env:PHOTON_DEV = 1; $env:PHOTON_LOG = 1; .\Photon.exe
start photon://selftest
```

Available sessions: `selftest`, `selftest-record`, `selftest-trim`, `selftest-trim-apply`, `selftest-gif`, `selftest-window`, `selftest-misc`, `selftest-webcam`, `selftest-settings`, `selftest-escape` and `selftest-plain`. Screenshots go to `%PHOTON_SHOTS%` (default `%TEMP%\photon-shots`).

The self-tests capture whatever is on screen and save real recordings, so delete those files afterwards. With `PHOTON_DEV` set, Photon's own panels are not hidden from captures, so they appear in the test screenshots.

## Uninstalling

1. Right-click the tray icon and turn off **Launch at Login**, then choose **Quit Photon**.
2. Delete `Photon.exe`.
3. Optionally delete `%APPDATA%\Photon`, `%LOCALAPPDATA%\Photon` and the registry key `HKCU\Software\Classes\photon`.
4. Your screenshots and recordings stay in their folders.
