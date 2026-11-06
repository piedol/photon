# Deployment

Photon has no servers or databases, so deploying it means building the app bundle, signing it and installing it on a Mac.

## Build and install

```bash
scripts/bundle.sh
```

```bash
scripts/bundle.sh --install
```

```bash
scripts/bundle.sh --notarize
```

The first command builds and signs into `dist/Photon.app`. `--install` also replaces `/Applications/Photon.app` and relaunches it. `--notarize` also notarizes and staples the app, zips it as `dist/Photon-macOS-arm64.zip` for updates, and makes `dist/Photon-macOS-arm64.dmg` for new installs.

What `bundle.sh` does:
1. `cargo build --release --target aarch64-apple-darwin`. The release profile uses fat LTO, 1 codegen unit, `panic = "abort"`, stripped symbols and opt-level 3.
2. Assembles `Photon.app`:
   - The binary.
   - `Info.plist`, with the version taken from `Cargo.toml` and the build number from the git commit count.
   - `AppIcon.icns`.
3. Signs with the hardened runtime and `resources/Photon.entitlements`:
   - It uses `PHOTON_SIGN_IDENTITY`, or else the first "Developer ID Application" certificate in your keychain.
   - Without either, it falls back to an ad-hoc signature and warns. macOS then asks for permissions again after every build.
4. Verifies the signature and prints the bundle size, binary size and architecture.

The bundle is about 1.9 MB (a 940 KB binary, the icon and the translations) and arm64 only.

## Notarizing

Notarizing is needed when giving Photon to another Mac. Create a keychain profile once, using the Apple ID that belongs to your developer team and an app-specific password from account.apple.com:

```bash
xcrun notarytool store-credentials piedol-notary --apple-id <your Apple ID> --team-id <your team ID>
```

Then run `scripts/bundle.sh --notarize`. It signs with a secure timestamp, submits the app, waits for Apple, staples the ticket and writes the release zip. Set `NOTARY_PROFILE` to use a profile with another name.

## Updating

A copy installed from a release updates from the menu (**Check for Updates…**) or after the daily check. See [Updates](../README.md#updates).

For a build from source, run `scripts/bundle.sh --install` again. Same-identity signing keeps Screen Recording, Microphone and Camera permissions and the login item. Settings in `~/Library/Preferences/top.piedol.photon.plist` are untouched.

## Launch at Login

Turn it on from the menu, from Settings › General, or with:

```bash
/Applications/Photon.app/Contents/MacOS/Photon --login-item on
```

## Uninstalling

1. Quit Photon (menu › Quit Photon).
2. Turn off Launch at Login first, or remove it later in System Settings › General › Login Items.
3. Move `/Applications/Photon.app` to the Trash.
4. Optionally run `defaults delete top.piedol.photon` to remove settings.
5. Optionally remove Photon from Privacy & Security › Screen & System Audio Recording, Microphone and Camera.
6. Your screenshots and recordings stay in their folders (`~/Pictures/Photon`, `~/Movies/Photon` or your chosen ones).

## Releasing to GitHub

### The disk image

`scripts/make-dmg.sh` builds the disk image with [dmgbuild](https://github.com/dmgbuild/dmgbuild), which lays out the window without scripting Finder. Install it once with `python3 -m venv ~/.local/share/dmgbuild-venv && ~/.local/share/dmgbuild-venv/bin/pip install dmgbuild`, or set `DMGBUILD` to its path. `scripts/dmg-settings.py` places the icons and hides the toolbar and sidebar, and `scripts/dmg-background.swift` draws the background in Photon's accent colour, toned to a mid grey so Finder's labels read in Light and Dark Mode. The image is signed, notarized and stapled. The three scripts are shared with piedol's other Mac apps and kept the same in each.

The repo is `piedol/photon`. Releases are cut from `main`.

App builds are published as GitHub Releases tagged `vX.Y.Z` (the version in `Cargo.toml` and `windows/Cargo.toml`), each with three files:
- `Photon-macOS-arm64.zip`: `dist/Photon.app` from `scripts/bundle.sh --notarize`, zipped with `ditto -c -k --keepParent`. Installed copies update from this file.
- `Photon-macOS-arm64.dmg`: the disk image for new installs, also from `scripts/bundle.sh --notarize`. It opens to a window with Photon, an arrow and the Applications folder. The README and piedol.top link to it.
- `Photon-windows-x64.zip`: `windows\target\release\Photon.exe` plus `LICENSE`, built on Windows with `cargo build --release` (see [windows.md](windows.md)).

```bash
gh release create v0.1.0 Photon-macOS-arm64.zip Photon-macOS-arm64.dmg Photon-windows-x64.zip --title "Photon 0.1.0" --notes-file notes.md
```

Installed copies update from the latest release, so:
- Keep the asset names exactly as above, and the tag as `vX.Y.Z`. Drafts and pre-releases are ignored.
- GitHub lists a SHA-256 `digest` for each asset. Photon won't install an asset that has no digest.
- The Mac zip must hold a notarized and stapled `Photon.app` (`scripts/bundle.sh --notarize`) with a higher `CFBundleShortVersionString`.
- The Windows exe's version resource (`windows/resources/photon.rc`) must match `windows/Cargo.toml` and name the product Photon.
- The release notes are shown in the update window. Headings, bullets, bold and code spans are formatted.

To try an update end to end, build a copy with a lower version, put it in a scratch folder and start it with `PHOTON_UPDATE_TEST=install` (see [Configuration](configuration.md#environment-variables)).
