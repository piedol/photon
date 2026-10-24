# Photon

Screenshots, markup and screen recording for Apple silicon Macs. Photon is one native Rust menu-bar app. The binary is under 1 MB and it starts at about 12 MB of memory.

- **⇧⌘9**: freeze the screen, drag a region, mark it up, then copy, save, print or pin it.
- **⇧⌘0**: record a region, a single window or a whole display to MP4, with system audio, mic and an optional webcam bubble. Afterwards you can trim it or make a GIF.

Photon is local only. It has no accounts, no uploads and no network code. "Cloud sync" means saving into a OneDrive, Google Drive, Dropbox, Box or iCloud Drive folder, and that provider's own app uploads the file.

**Platform:** macOS 15+ on Apple silicon (arm64 only). It's written in Rust against AppKit, ScreenCaptureKit, AVFoundation and CoreGraphics through the `objc2` crates.

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

The standard pages for servers, databases, cloud infrastructure and day-2 operations don't apply. Photon is a single local app with no backend, database or hosted service. Operational notes live in [Deployment](docs/deployment.md) and [Troubleshooting](docs/troubleshooting.md).

## Quick install

```bash
scripts/bundle.sh --install
```

Then allow Screen & System Audio Recording when macOS asks. See [Quickstart](docs/quickstart.md) for the details.

## Docs update tag

Docs last updated: `docs-2026-10-24`

Docs audited through commit: `f44a970`

This tag marks the last commit where `README.md` and `docs/` were audited and updated.
- View changes since the docs were last updated: `git log docs-2026-10-24..HEAD --oneline`
- Diff the docs against now: `git diff docs-2026-10-24..HEAD -- README.md docs/`

Update procedure:
1. Pick a new tag name like `docs-YYYY-MM-DD`, or `docs-YYYY-MM-DD-2` if you've already used today's tag.
2. Before changing any docs, set `Docs audited through commit` to the code commit you're auditing: `git rev-parse --short HEAD`.
3. Update the `Docs last updated` line in the same commit as your docs changes.
4. Commit with: `docs: sync docs (docs-YYYY-MM-DD)`.
5. Create the annotated tag: `git tag -a docs-YYYY-MM-DD -m "docs: sync docs"`.
6. Push the tag: `git push origin docs-YYYY-MM-DD`.
