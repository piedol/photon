# Security & privacy

## What Photon can see

- **Your screen,** but only when you ask. Displays are captured when the overlay opens. The stream runs only between Record and Stop.
- **System audio, microphone and camera,** only during a recording, and only when that toggle is on.
- **The window list** (positions, owning app names) when the overlay opens, to make windows clickable.
- **The front app's name,** used for the `{app}` naming token.

Nothing is captured in the background, and nothing is kept after the overlay closes, apart from what you copy or save. See [Storage](storage.md).

## What leaves the Mac

**Photon itself sends nothing.** There's no network code, no analytics, no crash reporter and no update checker.

Files can leave the Mac only through things you choose:
- A **cloud folder** you selected. The provider's app uploads it under your account.
- The **clipboard,** which other apps (and Universal Clipboard) can read.
- **Copy file,** which puts a file reference on the clipboard.

## Sensitive content

- Use **Pixelate** before copying. Pixelation is burned into the exported image; the original pixels aren't kept anywhere.
- **Single-window recordings** show only that window. Notifications, other apps and anything else on screen aren't recorded.
- Region and display recordings include anything that appears in that area. Photon's own outline, HUDs and toolbars are always excluded; only the webcam bubble is included on purpose.

## Secrets

Photon stores no passwords, tokens or keys. The only secret involved is your **code-signing identity**, which stays in the macOS keychain. `scripts/bundle.sh` refers to it by name only.

Notarization credentials, if you use them, live in a `notarytool` keychain profile, never in the repo.

## Supply chain

Dependencies are Apple-framework bindings (`objc2-*`, `block2`, `dispatch2`), `global-hotkey` and `libc`, with versions pinned by `Cargo.lock`. The app is a single signed binary with no plugins, scripts or dynamic downloads.
