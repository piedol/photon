# Architecture

Photon is one Rust binary (about 8,000 lines across `src/`). It's a menu-bar AppKit app: accessory activation policy, no Dock icon. All UI runs on the main thread. ScreenCaptureKit and AVFoundation callbacks arrive on their own queues and hop back to the main queue (`util::main_async`) before touching state.

## Modules

| Module | Role |
|---|---|
| `main.rs` | Entry point. Handles `--login-item`, otherwise `app::run()` |
| `app.rs` | Launch, menu-bar item, recording clock, login item, command routing (`handle`) |
| `commands.rs` | `Command` enum. Every button and menu item sends its command as an integer tag. |
| `ui.rs` | The shared `PhotonController` ObjC object that every control targets, plus symbol, button and HUD helpers |
| `hotkey.rs` | Global shortcuts and the temporary Esc shortcut |
| `capture.rs` | Freeze-frame capture of every display (ScreenCaptureKit), window list for snapping (CGWindowList) |
| `session.rs` | One capture session: overlays, selection, markup, actions |
| `overlay/` | `window.rs` (key-capable panel and event view), `chrome.rs` (CAShapeLayer dim, border and handles), `selection.rs` (pure state machine), `toolbar.rs` (palette, action bar, record bar, badge) |
| `annotate/` | `model.rs` (shapes, drafts, undo; pure), `render.rs` (CoreGraphics drawing shared by screen and export) |
| `export.rs` | Compose the final image; copy, save, print |
| `pin.rs` | Floating pinned captures |
| `record/` | `mod.rs` (recorder state machine), `indicator.rs` (outline, countdown), `webcam.rs`, `post.rs` (segment joining, saved HUD, trim, GIF) |
| `output/` | `destination.rs` (folders, fallback, atomic writes), `naming.rs` (templates; pure), `cloud.rs` (cloud folder discovery; pure) |
| `settings.rs`, `settings_window.rs` | Preferences model and its window |
| `permissions.rs` | Screen, mic and camera permission checks |
| `hud.rs` | Toasts and HUD panels |
| `keys.rs`, `geometry.rs`, `util.rs` | Key tables, coordinate maths, helpers |
| `selftest.rs` | Developer-only scripted sessions |

## Screenshot pipeline

```text
hotkey ─► session::start ─► capture::capture_all (SCK, async, all displays)
                                   │ CGImage per display + window list
                                   ▼
                     session::open: one OverlayWindow per display
                       ├─ image view (layer.contents = frozen CGImage, GPU)
                       ├─ Chrome (CAShapeLayers: dim / hover / border / handles)
                       ├─ AnnotView (drawRect → annotate::render, clipped to selection)
                       └─ Toolbars (attached to the active display's overlay)
mouse/keys ─► Selection state machine / Draft shapes ─► Document
Copy/Save/Print/Pin ─► export::compose (crop + render shapes at pixel scale)
                      ─► pasteboard / file / NSPrintOperation / pin window
                      ─► session::close (windows closed, all buffers dropped)
```

Key decisions:
- **Freeze first.** Displays are captured before any Photon window appears, so no filtering is needed and the markup matches what you saw.
- **Vector markup.** Shapes are stored in view points. The same `render.rs` draws them on screen and into the export bitmap at native pixel scale. Pixelate samples the frozen frame.
- **GPU chrome.** Moving the selection only updates CAShapeLayer paths; the frozen frame is never repainted.
- **Key-capable non-activating panel.** The overlay gets the keyboard even when macOS won't activate the app after a global hotkey.

## Recording pipeline

```text
record overlay ─► session::RecordTarget { display, source rect, window id? }
               ─► record::start: outline, webcam bubble, countdown (Esc cancels, Skip)
               ─► SCShareableContent ─► SCContentFilter
                     region/display: display minus Photon's windows (except the webcam bubble), sourceRect
                     window:         desktop-independent window filter (follows the window)
               ─► SCStream + SCRecordingOutput (hardware H.264/HEVC → MP4 segment in temp)
pause  = remove the recording output (closes segment)   resume = add a new output (new segment)
stop   = stopCapture ─► every segment finished ─► post::join (AVMutableComposition passthrough)
                     ─► Folder destination (named, fallback-aware) + optional second copy
                     ─► "Recording saved" HUD (Show in Finder / Copy file / Open / Trim / GIF)
```

Key decisions:
- **SCRecordingOutput** (macOS 15) writes encoded video straight to disk. RAM stays flat however long the recording is.
- **Pause** works through segments, joined without re-encoding.
- A segment counts as finished when the recording output reports it. Paused segments are marked finished 0.8 s after removal. A 6 s safety net finishes a stop even if a callback never arrives.
- **Click highlights** come from ScreenCaptureKit's own `showMouseClicks`.

## Memory lifecycle

- **Idle:** status item, hotkeys and the controller only. About 12 MB.
- **During a capture:** the frozen frames dominate (about 40 MB per 5K display, plus a GPU copy). Everything is owned by the `Session` and dropped when it closes.
- **After closing:** `util::release_memory_soon` calls `malloc_zone_pressure_relief` one second later, so freed buffers go back to the system. Measured footprint settles at about 45 MB after the first capture (AppKit and graphics caches) and stays flat over repeated captures.

## Threading rules

- Never touch AppKit off the main thread. Background callbacks capture only `Send` data, or wrap main-thread values in `util::MainBound` and hand them to `main_async`.
- Session, recording and settings state live in `thread_local!` `RefCell`s on the main thread. Event handlers use `try_borrow_mut` (`with`) and do re-entrant work, like closing the session, only after the borrow ends.
