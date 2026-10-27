# Security & privacy

## What Photon can see

- Your screen, only when you ask. Displays are captured when the overlay opens. The recording stream runs only between Record and Stop.
- System audio, the microphone and the camera, during a recording and only when that toggle is on.
- The window list (positions, owning app names) when the overlay opens, to make windows clickable.
- The front app's name, for the `{app}` naming token.

Photon captures nothing in the background. When the overlay closes, it keeps only what you copied or saved. See [Storage](storage.md).

## What leaves the Mac

Photon connects to GitHub and nowhere else:
- About a minute after launch if the last check was more than a day ago, then daily while it runs, it asks `api.github.com` for the latest release of `piedol/photon`. The request carries the user agent `Photon/<version>` and nothing about you or your files. To turn this off, clear **Check for updates automatically** in Settings › General. **Check for Updates…** in the menu still works.
- When you click **Install and Relaunch**, it downloads that release's zip from GitHub.

Before an update replaces anything, the zip's SHA-256 must match the `digest` GitHub lists for the release asset (a release without a digest isn't installed). The app inside must then pass `anchor apple generic and identifier "top.piedol.photon" and certificate leaf[subject.OU] = "D9JNQPYFBM" and notarized` under strict validation, and be newer than the running app. On Windows, where `Photon.exe` isn't Authenticode-signed, the checksum fetched over HTTPS is the check, and the new exe's version resource must name Photon and a newer version. Requests go through macOS's NSURLSession and Windows' WinHTTP, so Photon has no TLS code of its own.

Files can leave the Mac only through things you choose:
- A **cloud folder** you selected. The provider's app uploads it under your account.
- The **clipboard,** which other apps (and Universal Clipboard) can read.
- **Copy file,** which puts a file reference on the clipboard.

## Sensitive content

- Use **Pixelate** before copying. Pixelation is burned into the exported image, and the original pixels aren't kept anywhere.
- A window recording shows only that window. Notifications, other apps and the rest of the screen aren't recorded.
- Region and display recordings include anything that appears in that area. Photon's outline, HUDs and toolbars are always excluded. The webcam bubble is the only Photon window that's recorded.

## Secrets

Photon stores no passwords, tokens or keys. The only secret involved is your **code-signing identity**, which stays in the macOS keychain. `scripts/bundle.sh` refers to it by name only.

Notarization credentials, if you use them, are stored in a `notarytool` keychain profile, never in the repo.

## Supply chain

Dependencies are Apple-framework bindings (`objc2-*`, `block2`, `dispatch2`), `global-hotkey`, `libc` and `serde_json` (for GitHub's release JSON), with versions pinned by `Cargo.lock`. The app ships as a signed binary with no plugins or scripts. The only code it downloads is a release you choose to install, checked as above.
