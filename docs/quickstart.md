# Quickstart

## Requirements

- An Apple silicon Mac (M1 or later) on **macOS 15 or later**. The build refuses any target other than `aarch64-apple-darwin`, and the app bundle requires macOS 15 (`LSMinimumSystemVersion`).
- Rust stable (built and tested with 1.98) and the Xcode command line tools.
- Optional: a "Developer ID Application" signing identity in your keychain. Without it, the app is ad-hoc signed and macOS forgets its permissions on every rebuild. See [Deployment](deployment.md).

## Build and install

```bash
scripts/bundle.sh --install
```

This:
1. Builds a release binary for `aarch64-apple-darwin`.
2. Assembles `dist/Photon.app`.
3. Signs it with the hardened runtime.
4. Replaces `/Applications/Photon.app`.
5. Launches it.

Photon lives in the menu bar (the crop-corners icon) and has no Dock icon.

## First launch

1. macOS asks for **Screen & System Audio Recording**. Allow it. If you dismissed the prompt, Photon shows a dialog with an **Open System Settings** button. Turn on Photon under Privacy & Security › Screen & System Audio Recording.
2. Press **⇧⌘9** to take a screenshot, or **⇧⌘0** to record.
3. To start Photon at login, use **Launch at Login** in the menu, or Settings › General.

Microphone and Camera permissions are only requested the first time you turn on the mic or the webcam bubble.

## Shortcut conflicts

Other screenshot tools may also use ⇧⌘9. Quit them, or change Photon's shortcut in Settings › General.

## On Windows

Download `Photon-windows-x64.zip` from Releases, or build it yourself. See [Photon for Windows](windows.md).

## Next

- [Usage](usage.md) covers every tool and key.
- [Configuration](configuration.md) covers where files go and how they're named.
