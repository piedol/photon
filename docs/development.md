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

To test the signed app (so permissions stick), run:

```bash
scripts/bundle.sh --install
```

## Self-tests

Synthetic mouse and keyboard events need Accessibility permission, so Photon has built-in scripted sessions instead. Start the installed binary with developer mode on:

```bash
PHOTON_DEV=1 PHOTON_LOG=1 /Applications/Photon.app/Contents/MacOS/Photon
```

Then open a self-test URL:

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
| `photon://selftest-showcase` | The README's overlay shots, over the page from `swift scripts/showcase-backdrop.swift` |
| `photon://selftest-settings-shots` | Each Settings tab as a window capture without its shadow |

Screenshots go to `$PHOTON_SHOTS` (default `/tmp/photon-shots`), and logs go to stderr.

The self-tests capture whatever is on screen, and the recording tests save files to your recordings folder. Delete those files afterwards.

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

- AppKit is called on the main thread only. Background callbacks hop back with `util::main_async`, and main-thread values cross in `util::MainBound`.
- State is kept in `thread_local!` `RefCell`s. Use the module's `with()` (try-borrow) and do re-entrant work, such as closing windows, after the borrow ends.
- A new button gets a `Command` variant with a unique tag in `commands.rs` (and the round-trip test), and is handled in `app::handle` or `session::command`.
- objc2 classes use `define_class!`. Remember `set_ivars(())` before `msg_send![super(this), init…]`.
- Pure logic (geometry, selection, model, naming, cloud) stays free of AppKit and gets unit tests.

## Adding a markup tool

1. Add a variant to `annotate::model::Tool` (and to `ALL`, `symbol`, `tooltip`).
2. Add a `Shape` variant, then handle it in `Draft::update`, `Draft::shape` and `Draft::finish`.
3. Draw it in `annotate::render::draw_shape`. It's then automatically on screen and in exports.
4. The palette picks it up from `Tool::ALL` and grows by a button's height.

## Translations

Photon is translated into Simplified Chinese, Japanese, German, French, Spanish, Brazilian Portuguese and Korean. The Mac app uses the language macOS picks for it (your preferred languages in System Settings › General › Language & Region, or the one set for Photon there). The Windows edition follows Windows' display language, and `PHOTON_LANG=de` overrides it.

Text the user sees goes through the calls in `src/i18n.rs` (Mac) and `windows/src/i18n.rs` (Windows), with the English text as the key:

```rust
tr("Quit Photon")                                // fixed text
trf("Saved to {folder}", &[("folder", &name)])  // values in the text, by name
trn(count, "{n} item", "{n} items")          // counts; both forms are literal
```

- Keys must be string literals, so the test below can find them. Translate whole sentences: `trf("Saved to {folder}", …)`, never `tr("Saved to") + &name`, because word order differs between languages.
- Placeholders are named, `{folder}` rather than `{}`, so a translation can move them.
- Leave untranslated: product and brand names, file names, URL schemes, settings keys, log and debug output, and the self-tests.
- Each language is a flat JSON object in `locales/<lang>.json`, from the English text to the translation. `scripts/bundle.sh` turns each into `<lang>.lproj/Localizable.strings` in the app bundle. The Windows edition compiles the same catalogs in. The camera and microphone permission prompts come from `resources/Info.plist`; `bundle.sh` translates them into `InfoPlist.strings` from the same catalogs.

`cargo test` runs `catalogs_match_the_source`, which fails when a catalog is missing a key, still has a key the code no longer uses, or has different placeholders from the English. Add the new English text to every catalog when you add a string.

To see the app in a language without changing your Mac's settings:

```bash
open /Applications/Photon.app --args -AppleLanguages '(de)'
```

## Project layout

```text
src/            Rust sources (see architecture.md)
windows/        the Windows edition, a separate crate (see windows.md)
resources/      Info.plist template, entitlements
assets/         AppIcon.icns and icon.png (generated), README images
scripts/        bundle.sh (build/sign/install), make-icon.swift (renders the icon)
docs/           this documentation
```

To regenerate the icon after editing `scripts/make-icon.swift`:

```bash
swift scripts/make-icon.swift
```

## Keeping the docs in sync

When behaviour changes, update `README.md` and the matching page in `docs/` in the same pull request.
