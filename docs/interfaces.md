# Interfaces

Photon has no network API. Its interfaces are global shortcuts, keys inside the overlay, a URL scheme and the `--login-item` command-line flag.

## Global shortcuts

They're registered as Carbon hotkeys through the `global-hotkey` crate, which doesn't need Accessibility permission.

| Shortcut (default) | When idle | While recording |
|---|---|---|
| ⇧⌘9 | Screenshot overlay | Stop and save |
| ⇧⌘0 | Record overlay | Stop and save |
| Esc | Not claimed | Not claimed |

Pressing the same shortcut while its overlay is open closes the overlay. **Esc is claimed system-wide only while a selection overlay or a recording countdown is on screen**, so it always cancels them. It's released as soon as they close. See [Permissions](permissions.md#the-temporary-esc-shortcut).

## Keys inside the overlay

| Key | Action |
|---|---|
| Esc | Cancel text editing, then close the swatches, then close the overlay |
| ↩ / Enter | Copy and close (screenshot), or start recording (record) |
| ⌘C, ⌘S, ⌘P | Copy, Save, Print |
| ⌘Z | Undo the last mark. While typing, it undoes text in the text box. The Undo button cancels the text instead. |
| ← → ↑ ↓ | Move the selection 1 px (⇧ for 10 px). ⌥ resizes instead. |
| `[` `]` or scroll | Smaller or larger stroke |
| ⇧ while drawing | Constrain to 45°, square or circle |

The overlay is a non-activating panel, so it receives these keys even when macOS doesn't bring Photon to the front.

## URL scheme

```bash
open photon://capture
```

| URL | Same as |
|---|---|
| `photon://capture` (or `photon://screenshot`) | ⇧⌘9 |
| `photon://record` | ⇧⌘0 |
| `photon://stop` | Stop and save a recording |
| `photon://settings` | Open Settings |

Developer-only URLs (`selftest`, `selftest-record`, `selftest-window`, `selftest-escape`, `selftest-settings`) do nothing unless Photon was started with `PHOTON_DEV` set. See [Development](development.md). Unknown hosts are ignored.

These URLs work from Raycast, Alfred, Shortcuts (Open URL) and the terminal.

## Command line

```bash
/Applications/Photon.app/Contents/MacOS/Photon --login-item on
```

The argument can be `on`, `off` or `status`. It prints `Launch at Login: on|off` and exits with:
- `0` on success.
- `1` with `Couldn't change Launch at Login: …` on stderr if macOS refuses.

Without arguments, the binary runs the menu-bar app.

## How errors appear

Photon has no error codes for users. Problems appear for about 4 seconds as a HUD next to the pointer, with an orange warning symbol, a title and a line of detail. The messages are:

| Message | Cause |
|---|---|
| Couldn't capture the screen | ScreenCaptureKit returned an error, usually missing permission |
| Recording stopped | The stream failed mid-recording. Whatever was recorded is still saved. |
| Couldn't save the recording | Joining segments or moving the file into the folder failed |
| Copied, but auto-save failed | The PNG couldn't be written. The clipboard copy still worked. |
| Copied · cloud folder missing / Saved locally · cloud folder missing | The chosen cloud folder is unavailable, so the file went to the local default |
| Couldn't trim the recording / Couldn't make the GIF | AVFoundation export or ImageIO failed |
| Microphone access is off / Camera access is off | Permission denied. Photon also opens the right Settings pane. |
| Webcam bubble is off for window recordings | Window recordings can't include the bubble |
| Couldn't change Launch at Login | macOS (SMAppService) refused |

Shortcut conflicts appear in Settings › General, under the shortcut buttons.
