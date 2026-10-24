# Development

## Build, test, lint

```bash
cargo build
```

```bash
cargo test
```

```bash
cargo clippy
```

`cargo build` makes a debug build of the binary at `target/debug/Photon`. Clippy should report no warnings.

The unit tests (58) cover the pure modules:
- Geometry.
- The selection state machine.
- Markup drafts and undo.
- Naming templates.
- Cloud folder discovery.
- Destinations.
- Shortcut encoding.
- Command tags.

The AppKit, ScreenCaptureKit and AVFoundation code is exercised by the self-tests below.

To test the real app (signed, so permissions stick), use:

```bash
scripts/bundle.sh --install
```

## Self-tests

Synthetic mouse and keyboard events need Accessibility permission, so Photon has built-in scripted sessions instead. Start the installed binary with developer mode on:

```bash
PHOTON_DEV=1 PHOTON_LOG=1 /Applications/Photon.app/Contents/MacOS/Photon
```

Then trigger one:

```bash
open photon://selftest
```

| URL | What it drives |
|---|---|
| `photon://selftest` | Screenshot: hover, select, every markup tool, swatches, copy, then the record overlay |
| `photon://selftest-record` | Region recording with pause/resume, stop, the saved HUD, trim and GIF |
| `photon://selftest-window` | Click-a-window recording, countdown Skip, stop through the screenshot shortcut's command |
| `photon://selftest-escape` | Esc cancels the overlay and the countdown, and nothing is saved |
| `photon://selftest-settings` | Opens Settings and screenshots each tab |

Screenshots go to `$PHOTON_SHOTS` (default `/tmp/photon-shots`), and logs go to stderr.

**Privacy note:** the self-tests capture whatever is on screen, and recording tests save real files to your recordings folder. Delete those artifacts afterwards.

Useful log lines:
- `overlay shown in N ms`: shortcut-to-overlay latency (about 100 ms warm, about 230 ms for the first capture after launch).
- `overlay key window: …`
- `segment started / finished`
- `finalizing N segment(s)`
- `saved <path>`
- `hotkey <id> pressed`

## Memory checks

```bash
footprint -p $(pgrep -x Photon) | grep phys_footprint
```

```bash
leaks $(pgrep -x Photon)
```

Expect about 12 MB right after launch and about 45 MB after the first capture, flat after that.

## Conventions

- **Main thread only for AppKit.** Background callbacks hop back with `util::main_async`; main-thread values cross in `util::MainBound`.
- **State** lives in `thread_local!` `RefCell`s. Use the module's `with()` (try-borrow) and do re-entrant work, such as closing windows, after the borrow ends.
- **Commands:** a new button gets a `Command` variant with a unique tag in `commands.rs` (and the round-trip test), and is handled in `app::handle` or `session::command`.
- **objc2 classes** use `define_class!`. Remember `set_ivars(())` before `msg_send![super(this), init…]`.
- **Pure logic** (geometry, selection, model, naming, cloud) stays free of AppKit and gets unit tests.

## Adding a markup tool

1. Add a variant to `annotate::model::Tool` (and to `ALL`, `symbol`, `tooltip`).
2. Add a `Shape` variant, then handle it in `Draft::update`, `Draft::shape` and `Draft::finish`.
3. Draw it in `annotate::render::draw_shape`. It's then automatically on screen and in exports.
4. The palette picks it up from `Tool::ALL`. Its height grows by one button.

## Project layout

```text
src/            Rust sources (see architecture.md)
windows/        the Windows edition, a separate crate (see windows.md)
resources/      Info.plist template, entitlements
assets/         AppIcon.icns (generated)
scripts/        bundle.sh (build/sign/install), make-icon.swift (renders the icon)
docs/           this documentation
```

To regenerate the icon after editing `scripts/make-icon.swift`:

```bash
swift scripts/make-icon.swift
```
