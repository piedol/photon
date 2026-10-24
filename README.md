# Photon

Screenshots, markup and screen recording for Apple silicon Macs. One native Rust binary under 1 MB that starts at about 14 MB of memory.

- **⇧⌘9** — drag a region, mark it up, then press ↩ to copy it.
- **⇧⌘0** — drag a region (or click a window or display) and record it.

## Screenshots

Press **⇧⌘9**. The screen freezes and dims. Then:

1. Pick a region:
   - **Drag** to select a region.
   - **Click** a window to select just that window.
   - **Click** empty desktop to select the whole display.
2. Adjust the selection:
   - Drag the handles to resize it.
   - Drag inside it to move it (when no tool is active).
   - Arrow keys nudge it by 1 px. Hold ⇧ for 10 px. Hold ⌥ to resize instead of move.
3. Mark it up with the tool palette on the right.
4. Finish with the action bar underneath.

| Tool | Notes |
|---|---|
| Pen, Marker | Freehand drawing. Hold ⇧ for a straight stroke. |
| Line, Arrow | Hold ⇧ to snap to 45°. |
| Rectangle, Ellipse | Hold ⇧ for a square or circle. |
| Text | Click to type. ↩ commits, esc cancels. |
| Pixelate | Drag over anything you want to hide. |
| Step | Click to drop numbered badges ①②③. |
| Color | 8 swatches plus the system color picker. |
| Stroke | Three sizes: `[` and `]` or the scroll wheel. |
| Undo | ⌘Z |

| Action | Key |
|---|---|
| Copy to clipboard | ⌘C or ↩ |
| Save as PNG or JPEG | ⌘S |
| Print | ⌘P |
| Pin to screen (floating) | — |
| Record this area | — |
| Cancel | esc or right-click |

Pinned captures float above other windows:
- Drag them to move.
- Scroll to change opacity.
- Double-click or press esc to close.
- Right-click for Copy, Save or Close.

## Recording

1. Press **⇧⌘0**, or **Record** in the screenshot action bar.
2. Choose what to record:
   - **Drag** a region to record that area of the screen.
   - **Click a window** to record just that window. The badge says "… window only". The recording follows the window and never shows other windows, even when you switch apps or windows cover it.
   - **Click** empty desktop to record the whole display.
3. Toggle the mic, system audio and webcam bubble in the record bar. The webcam bubble only works with regions and displays, not single windows.
4. Press **Record** or ↩.

While recording:
- A 3-second countdown runs first. Click **Skip** to start right away.
- The menu-bar icon becomes a red dot with a timer.
- A dashed outline marks the area. It isn't in the video.

To stop, click the menu-bar timer or press either shortcut (⇧⌘0 or ⇧⌘9) again. Right-click the timer to **Pause**, **Resume** or **Discard**.

Recordings are MP4 at 60 fps by default (H.264, or HEVC for smaller files), hardware-encoded by ScreenCaptureKit and written straight to disk. Pauses are cut out without re-encoding.

When a recording finishes, a pop-up offers:
- **Show in Finder**
- **Copy file**, which pastes straight into Discord or Slack
- **Open**
- **Trim**, using the macOS trimmer
- **Make GIF**: 12 fps, up to 960 px wide, first 60 s

Click highlights, the pointer, the countdown, the frame rate and the microphone or camera device are all set in Settings.

## Where files go

| | Default | Options |
|---|---|---|
| Screenshots | Clipboard only | Auto-save a PNG to a folder |
| Recordings | `~/Movies/Photon` | Any folder, plus an optional second copy |

**Cloud sync** means picking a folder that OneDrive, Google Drive, Dropbox, Box or iCloud Drive already syncs. Photon finds them in `~/Library/CloudStorage`, and the provider's own app does the upload. If a provider folder disappears (for example after you sign out), Photon saves to the local default and says so.

**File names** come from templates you can edit, one for screenshots and one for recordings:
- Tokens: `{type}`, `{date}`, `{time}`, `{app}`, `{size}`, `{display}` and `{counter}`.
- Four date formats and four time formats.
- Case: as typed, Title Case, lowercase or UPPERCASE.
- Word separator: space, dash or underscore.

Settings shows a live preview of the result.

## Scripting

```bash
open photon://capture
```

Also available: `photon://record`, `photon://stop` and `photon://settings`. These work from Raycast, Shortcuts or a terminal.

## Install

Requires Rust (stable), Xcode command line tools and macOS 15 or later on Apple silicon.

```bash
scripts/bundle.sh --install
```

This builds a release binary for `aarch64-apple-darwin` only, assembles `dist/Photon.app`, signs it with the Developer ID identity in your keychain (hardened runtime) and installs it to `/Applications`.

Options:
- `--notarize` also notarizes the app. It needs a `notarytool` keychain profile named `photon`.
- `PHOTON_SIGN_IDENTITY` overrides the signing identity.

Launch at Login is a checkbox in Settings › General. From a terminal you can also run `/Applications/Photon.app/Contents/MacOS/Photon --login-item on`, `off` or `status`.

On first launch macOS asks for **Screen & System Audio Recording** permission. Microphone and Camera permissions are requested only when you turn those on. Stable signing means rebuilds keep the permissions.

If another screenshot tool uses ⇧⌘9, quit it first. You can change either shortcut in Settings › General.

## Development

```bash
cargo test
```

Unit tests cover geometry, the selection state machine, markup, naming, cloud discovery and destinations.

```bash
cargo clippy
```

```bash
PHOTON_DEV=1 PHOTON_LOG=1 /Applications/Photon.app/Contents/MacOS/Photon
```

With the app running like that, `open photon://selftest` (also `selftest-record` and `selftest-settings`) runs scripted sessions. Each saves screenshots to `/tmp/photon-shots`. They only work when `PHOTON_DEV` is set.

| Path | What it does |
|---|---|
| `src/app.rs` | Menu bar, command routing, recording clock |
| `src/session.rs` | A capture session: overlays, selection, markup, actions |
| `src/overlay/` | Overlay windows, selection chrome, toolbars |
| `src/annotate/` | Markup shapes and CoreGraphics rendering |
| `src/export.rs` | Compose, copy, save, print |
| `src/record/` | Recording, indicators, webcam, trim and GIF |
| `src/output/` | Destinations, naming templates, cloud folders |
| `src/settings.rs`, `src/settings_window.rs` | Preferences and their UI |

Memory: each capture session is created on demand and dropped when it ends, and freed buffers are returned to the system shortly afterwards. Photon starts at about 14 MB. After the first capture it settles at about 46 MB, mostly system graphics and symbol caches, and stays flat from then on.
