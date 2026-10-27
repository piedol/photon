# Security & privacy

## What Photon can see

- Your screen, only when you ask. Displays are captured when the overlay opens. The recording stream runs only between Record and Stop.
- System audio, the microphone and the camera, during a recording and only when that toggle is on.
- The window list (positions, owning app names) when the overlay opens, to make windows clickable.
- The front app's name, for the `{app}` naming token.

Photon captures nothing in the background. When the overlay closes, it keeps only what you copied or saved. See [Storage](storage.md).

## What leaves the Mac

Photon has no network code, so it makes no network connections.

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

Dependencies are Apple-framework bindings (`objc2-*`, `block2`, `dispatch2`), `global-hotkey` and `libc`, with versions pinned by `Cargo.lock`. The app ships as a signed binary with no plugins, scripts or runtime downloads.
