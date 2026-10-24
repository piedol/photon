<p align="center">
  <img src="assets/icon.png" width="128" alt="Photon icon">
</p>

<h1 align="center">Photon</h1>

<p align="center">
  <b>Screenshots, markup and screen recording for Apple silicon Macs, in one tiny native app.</b><br>
  Press a key, drag, done.
</p>

<p align="center">
  macOS 15+ · Apple silicon · native Rust · under 1 MB · about 12 MB of memory · no network code
</p>

---

## As simple as it gets

**Screenshot:**
1. Press **⇧⌘9**. The screen freezes.
2. Drag over what you want, or just click a window.
3. Press **↩**. It's on your clipboard, ready to paste anywhere.

**Screen recording:**
1. Press **⇧⌘0**.
2. Drag a region, or click a window.
3. Press **↩**. You're recording.
4. Press **⇧⌘0** again to stop. The file is saved, with one-click buttons to share, trim or turn it into a GIF.

Changed your mind? **Esc** cancels a selection or a countdown at any point before recording starts.

There are no accounts and no windows to manage. Photon lives in your menu bar and stays out of the way until you press a key.

## Why Photon

Photon handles screenshots and screen recording in one app built only for Apple silicon.

| | A typical screenshot app | Photon |
|---|---|---|
| Memory while idle | ~560 MB (an Intel screenshot app under Rosetta) | ~12 MB at launch, about 45 MB after heavy use |
| Native to Apple silicon | No | Yes, arm64 only |
| App size | 1.3 MB + 487 MB | 0.8 MB binary |
| Screenshots and recording | Two apps | One app, one set of shortcuts |
| Where your files go | Their servers when you share | Only to folders you pick. Photon itself never touches the network. |

## Star features

### Screenshots in one gesture
- **Freeze-frame capture** of every display at full Retina resolution. What you mark up is exactly what was on screen.
- **Click any window** to select it exactly, or click the desktop for the whole display.
- **Fine-tune** with drag handles, or nudge with the arrow keys (1 px, or 10 px with ⇧). A live badge shows the size in pixels.
- **Finish your way:** ↩ copies, ⌘S saves PNG or JPEG, ⌘P prints, or **pin** it to float above your other windows as a reference.

### Markup that looks good
- **Pen, line, arrow, rectangle, ellipse, highlighter and text.** Hold ⇧ for perfect 45° lines, squares and circles.
- **Pixelate** passwords, emails or anything private before you share.
- **Numbered steps** (1, 2, 3…) for quick how-to guides.
- **8 colours plus any custom colour**, and three stroke sizes on `[` `]` or the scroll wheel. Photon remembers your last choice.
- Everything is drawn as vectors and baked in at full resolution, so arrows and text stay crisp on Retina screens.

### Record exactly what you mean
- **A region, a whole display, or one window.** Click a window and Photon records *only that window*. It follows the window, and other apps never appear in the video, even when you switch apps or windows cover it.
- **60 fps hardware encoding** (H.264, or HEVC for smaller files) written straight to disk, so even long recordings barely touch memory.
- **System audio, microphone and a webcam bubble**: each one is a single click in the record bar.
- **Click highlights** and the pointer, so viewers can follow along.
- **Countdown with Skip**, **pause and resume**, and a **menu-bar timer**. Click the timer or press either shortcut to stop.

### Share it in seconds
After every recording, a small panel offers:
- **Copy file**, which pastes straight into Discord, Slack, Mail or Finder.
- **Trim**, with the same trimmer QuickTime uses. It's lossless and instant.
- **Make GIF**: a looping GIF, ready for chats and READMEs.
- **Show in Finder** and **Open**.

### Files land where you want them
- **Cloud sync without accounts.** Photon detects your OneDrive, Google Drive, Dropbox, Box and iCloud Drive folders. Pick one, and the provider's own app uploads your files. If you're ever signed out, Photon falls back to a local folder and tells you.
- **Keep a second copy** of every recording somewhere else, such as locally and in the cloud.
- **File names your way:** templates with date, time, app, size, display and counter tokens, plus case and separator styles, and a live preview.

### Small, fast and private
- **Native Rust** on Apple's own frameworks: about 100 ms from key press to overlay.
- **Nothing kept in the background.** Each capture is created when you press the key and fully released when you're done.
- **No network code at all.** No analytics, no uploads, no update pings.
- **Scriptable:** run `open photon://capture`, `photon://record` or `photon://stop` from Raycast, Shortcuts or the terminal.

## Get started

```bash
scripts/bundle.sh --install
```

That builds, signs and installs Photon to `/Applications`. Allow **Screen & System Audio Recording** when macOS asks, then press **⇧⌘9**. Turn on **Launch at Login** from the menu-bar icon.

You need macOS 15 or later on an Apple silicon Mac (M1 or newer), plus Rust and the Xcode command line tools to build it. Full steps are in [Quickstart](docs/quickstart.md).

## Shortcut cheat sheet

| Key | What it does |
|---|---|
| **⇧⌘9** | Screenshot (stops a recording if one is running) |
| **⇧⌘0** | Record (press again to stop) |
| **↩** | Copy the screenshot, or start recording |
| **Esc** / right-click | Cancel |
| **⌘C · ⌘S · ⌘P · ⌘Z** | Copy · Save · Print · Undo |
| **← → ↑ ↓** | Nudge the selection (⇧ for 10 px, ⌥ to resize) |
| **⇧** while drawing | Straight lines, squares and circles |
| **[ ]** or scroll | Stroke size |

You can change both shortcuts in **Settings › General**.

## Documentation

| Page | What it covers |
|---|---|
| [Quickstart](docs/quickstart.md) | Build, install, first launch, permissions, shortcut conflicts |
| [Usage](docs/usage.md) | Screenshots, markup tools, pins, recording, trim, GIF, every key and button |
| [Configuration](docs/configuration.md) | Every setting, its stored key and default, naming templates, environment variables |
| [Interfaces](docs/interfaces.md) | Global shortcuts, overlay keys, `photon://` URLs, the `--login-item` flag, how errors appear |
| [Architecture](docs/architecture.md) | Modules, the capture and recording pipelines, threading, memory lifecycle |
| [Storage](docs/storage.md) | Where every byte lives: clipboard, folders, cloud folders, temp segments, preferences |
| [Permissions](docs/permissions.md) | macOS privacy permissions, entitlements, signing, the temporary Esc shortcut |
| [Security & privacy](docs/security.md) | What Photon can see, what leaves the Mac, and how |
| [Development](docs/development.md) | Tests, lint, self-tests, logging, project layout, adding a tool |
| [Deployment](docs/deployment.md) | Bundling, signing, notarizing, installing, login item, uninstalling |
| [Troubleshooting](docs/troubleshooting.md) | Common problems and what to check |

Other records in the repo:

Photon is a single local app with no backend, database or hosted service, so the usual server, database and infrastructure pages don't apply.

## License

Photon is source-available under the [PolyForm Noncommercial License 1.0.0](LICENSE.md).

- **Allowed:**
  - Personal, hobby, research and educational use.
  - Use by nonprofits, schools and government.
  - Forking, modifying and sharing for any of those purposes.
- **Not allowed:** any commercial use, including use inside a company.
- **Credit required:** anyone who shares Photon or a fork must include the license (or its URL) and this line, unchanged:

  > Required Notice: Copyright (c) 2026 piedol (https://github.com/piedol/photon)

For commercial licensing, contact [piedol](https://github.com/piedol).

## Docs update tag

Docs last updated: `docs-2026-10-24-2`

Docs audited through commit: `1afd6b7`

This tag marks the last commit where `README.md` and `docs/` were audited and updated.
- View changes since the docs were last updated: `git log docs-2026-10-24-2..HEAD --oneline`
- Diff the docs against now: `git diff docs-2026-10-24-2..HEAD -- README.md docs/`

Update procedure:
1. Pick a new tag name like `docs-YYYY-MM-DD`, or `docs-YYYY-MM-DD-2` if you've already used today's tag.
2. Before changing any docs, set `Docs audited through commit` to the code commit you're auditing: `git rev-parse --short HEAD`.
3. Update the `Docs last updated` line in the same commit as your docs changes.
4. Commit with: `docs: sync docs (docs-YYYY-MM-DD)`.
5. Create the annotated tag: `git tag -a docs-YYYY-MM-DD -m "docs: sync docs"`.
6. Push the tag: `git push origin docs-YYYY-MM-DD`.
