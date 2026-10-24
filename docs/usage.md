# Usage

## Screenshots (⇧⌘9)

The screen freezes and dims. Photon captures every display at its native pixel resolution *before* drawing anything, so what you mark up is exactly what was on screen.

### Choosing what to capture

| Do this | You get |
|---|---|
| Drag | That region. A badge shows its size in pixels. |
| Click a window | That window's area. The highlight shows which window you're pointing at. |
| Click empty desktop | The whole display |
| Drag a handle | Resize the selection |
| Drag inside the selection (no tool active) | Move the selection |
| Arrow keys | Move the selection 1 px. Hold ⇧ for 10 px. |
| ⌥ + arrow keys | Resize from the right and top edges |
| Esc or right-click | Cancel and close |

A selection stays on the display where you started it. After you add any markup, dragging outside the selection no longer starts a new one, so a stray drag can't throw your work away.

### Markup tools (palette to the right of the selection)

| Tool | How it works |
|---|---|
| Pen | Freehand. Hold ⇧ for a straight stroke. |
| Line, Arrow | Hold ⇧ to snap to 45°. |
| Rectangle, Ellipse | Hold ⇧ for a square or circle. |
| Marker | Translucent highlighter. Hold ⇧ for a straight stroke. |
| Text | Click where the text should start and type. ↩ commits, Esc cancels. Clicking elsewhere also commits. |
| Pixelate | Drag over anything to hide it. The pixels come from the frozen screenshot. |
| Step | Each click drops the next numbered badge (1, 2, 3…). Undo frees the number again. |
| Colour | The colour dot opens 8 swatches plus a picker for any colour. Your choice is remembered. |
| Stroke | Three sizes. Click the dot to cycle, press `[` and `]`, or scroll. Your choice is remembered. |
| Undo | ⌘Z or the arrow button |

Click a tool again to turn it off and get back to moving the selection. Markup is drawn only inside the selection.

### Finishing (bar under the selection)

| Button | Key | What happens |
|---|---|---|
| Copy | ⌘C or ↩ | PNG to the clipboard (Retina images carry 144 dpi). If **auto-save** is on, a PNG is also written to your screenshot folder. |
| Save | ⌘S | Save panel. The name is pre-filled from your naming template and the last folder is remembered. Ends in `.jpg` or `.jpeg` to get a JPEG; otherwise PNG. |
| Print | ⌘P | System print dialog, scaled to fit the page and centred |
| Pin | — | Floats the capture above other windows (see below) |
| Record | — | Starts recording this area. If you clicked a window, only that window is recorded. |
| ✕ | Esc | Cancel |

A small "Copied" or "Saved" confirmation appears next to the pointer.

### Pinned captures

- Drag to move.
- Scroll to change opacity (25–100%).
- Double-click or press Esc to close.
- ⌘C copies it.
- Right-click for Copy, Save… or Close.

Pins stay until you close them or quit Photon. They're not saved anywhere.

## Recording (⇧⌘0)

1. Choose what to record, the same way as screenshots:
   - **Drag** a region to record that part of the screen.
   - **Click a window** to record *only that window*. The badge reads "… window only". The video follows the window and never shows other windows, even when you switch apps or windows cover it.
   - **Click empty desktop** to record the whole display.
2. In the record bar, toggle **microphone**, **system audio** and the **webcam bubble**. These toggles are remembered. The webcam bubble isn't available when recording a single window.
3. Press **Record** or ↩.

### While recording

- A 3-second countdown appears. **Skip** starts immediately; **Esc** cancels and nothing is saved.
- Region and display recordings show a dashed outline around the area: red while recording, grey while paused. The outline is never in the video. Window recordings have no outline, because the window can move.
- The menu-bar icon becomes a **red dot and a clock**.
  - **Click the clock** to stop and save.
  - **Right-click** for Stop and Save, Pause or Resume, and Discard Recording.
- Pressing **either shortcut (⇧⌘0 or ⇧⌘9)** stops and saves.
- The webcam bubble shows your camera in a circle in the corner of the area. Drag it anywhere; it *is* in the video.
- Clicks are highlighted and the pointer is shown, unless you turn those off in Settings.
- Quitting Photon while recording saves the recording first.

### After a recording

A panel appears in the bottom-right for 10 seconds:

| Button | What happens |
|---|---|
| Show in Finder | Reveals the file |
| Copy file | Puts the file on the clipboard, ready to paste into Discord, Slack, Mail or Finder |
| Open | Opens the file in your default video player |
| Trim | Opens the macOS trimmer (yellow handles). **Trim** replaces the file with the trimmed version, without re-encoding. |
| Make GIF | Writes a looping GIF next to the video: 12 fps, up to 960 px wide, first 60 seconds |

The GIF gets its own panel with Show in Finder, Copy file and Open.

## Menu-bar menu

- Capture Screenshot
- Record Screen
- Open Recordings Folder
- Settings… (⌘,)
- Launch at Login
- Quit Photon (⌘Q)
