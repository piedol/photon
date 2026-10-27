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

The first command builds and signs into `dist/Photon.app`. `--install` also replaces `/Applications/Photon.app` and relaunches it. `--notarize` also notarizes and staples the app, then zips it as `dist/Photon-macOS-arm64.zip` for a release.

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

The bundle is about 1.7 MB (an 824 KB binary, the icon and the translations) and arm64 only.

## Notarizing

Notarizing is needed when giving Photon to another Mac. Create a keychain profile once, using the Apple ID that belongs to your developer team and an app-specific password from account.apple.com:

```bash
xcrun notarytool store-credentials piedol-notary --apple-id <your Apple ID> --team-id <your team ID>
```

Then run `scripts/bundle.sh --notarize`. It signs with a secure timestamp, submits the app, waits for Apple, staples the ticket and writes the release zip. Set `NOTARY_PROFILE` to use a profile with another name.

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

The repo is `piedol/photon`. Releases are cut from `main`.

App builds are published as GitHub Releases tagged `vX.Y.Z` (the version in `Cargo.toml` and `windows/Cargo.toml`), each with two files:
- `Photon-macOS-arm64.zip`: `dist/Photon.app` from `scripts/bundle.sh`, zipped with `ditto -c -k --keepParent dist/Photon.app Photon-macOS-arm64.zip`.
- `Photon-windows-x64.zip`: `windows\target\release\Photon.exe` plus `LICENSE`, built on Windows with `cargo build --release` (see [windows.md](windows.md)).

```bash
gh release create v0.1.0 Photon-macOS-arm64.zip Photon-windows-x64.zip --title "Photon 0.1.0" --notes-file notes.md
```
