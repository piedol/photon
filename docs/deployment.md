# Deployment

Photon is a single local app with no servers, databases or hosted infrastructure. "Deployment" means building the app bundle, signing it and installing it on a Mac.

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

The first command builds and signs into `dist/Photon.app`. `--install` also replaces `/Applications/Photon.app` and relaunches it. `--notarize` also notarizes and staples the app.

What `bundle.sh` does:
1. `cargo build --release --target aarch64-apple-darwin`. The release profile uses fat LTO, 1 codegen unit, `panic = "abort"`, stripped symbols and opt-level 3.
2. Assembles `Photon.app`:
   - The binary.
   - `Info.plist`, with the version taken from `Cargo.toml` and the build number from the git commit count.
   - `AppIcon.icns`.
3. Signs with the hardened runtime and `resources/Photon.entitlements`:
   - It uses `PHOTON_SIGN_IDENTITY`, or else "Developer ID Application: Your Name (TEAMID)".
   - If that identity isn't in the keychain, it falls back to an ad-hoc signature and warns. Permissions then reset on every build.
4. Verifies the signature and prints the bundle size, binary size and architecture.

Result: a ~1.5 MB bundle (about 800 KB binary plus the icon), arm64 only.

## Notarizing (optional)

Notarizing is only needed when giving Photon to another Mac. First create a keychain profile once:

```bash
xcrun notarytool store-credentials photon --apple-id <you> --team-id <your team ID>
```

Then run `scripts/bundle.sh --notarize`. It zips the app, submits it, waits, and staples the ticket.

## Updating

Run `scripts/bundle.sh --install` again. Same-identity signing keeps Screen Recording, Microphone and Camera permissions and the login item. Settings in `~/Library/Preferences/top.piedol.photon.plist` are untouched.

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

The repo is `piedol/photon` (private), with `main` as the branch. Docs syncs are tagged `docs-YYYY-MM-DD`; see the README.
