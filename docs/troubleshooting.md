# Troubleshooting

## The shortcut does nothing

**Another app owns it.** Some other screenshot tools also use ⇧⌘9.
- Quit the other app, or pick a new shortcut in Settings › General.
- If registration fails, the note under the shortcut buttons says so.

**Photon isn't running.** Look for the crop-corners icon in the menu bar, or check:

```bash
pgrep -x Photon
```

**Developer check:** run the binary with `PHOTON_LOG=1`. Each press logs `hotkey <id> pressed`.

## The overlay opens but the screen is black, or capture fails

Screen Recording permission is missing or stale:
- Check System Settings › Privacy & Security › Screen & System Audio Recording.
- After an **ad-hoc** rebuild, remove Photon from that list and add it again. A stable Developer ID signature avoids this. See [Deployment](deployment.md).

## Esc, ↩ or typing doesn't work in the overlay

The overlay is a key-capable panel, and Esc is also claimed system-wide while it's open, so this shouldn't happen. If it does:
- Click once inside the overlay.
- Check whether another app has grabbed Esc as a global hotkey.

## "Bypass the system private window picker" prompt

This is macOS's periodic consent prompt for apps that use ScreenCaptureKit directly. Choose **Allow**.

If it names **Terminal** or another app instead of Photon, Photon was started from a shell. See [Permissions](permissions.md).

## Recording

| Symptom | Check |
|---|---|
| Other windows appear in the video | You dragged a region. Click the window instead, so the badge says "… window only". |
| No microphone in the file | The mic toggle was off, or Microphone permission was denied. Pick the right device in Settings › Recordings. |
| No system audio | Turn on "Record system audio". Photon's own sounds are always excluded. |
| No webcam bubble | Camera permission and the camera picker. Window recordings can't include the bubble. |
| "Recording stopped" mid-way | The stream failed, for example because the display disconnected or the window closed. What was recorded is still saved. |
| Stop took a few seconds | It finishes within 6 s even if macOS drops a callback. Run with `PHOTON_LOG=1` to see segment events. |
| A `photon-recordings` folder in the temp directory | Leftover segments from a crash. They're deleted automatically at next launch. |

## Files went to ~/Pictures/Photon or ~/Movies/Photon instead of the cloud

The cloud provider's folder was missing, for example because you were signed out or the app was uninstalled. Sign back in to the provider's app, or choose another folder in Settings. See [Storage](storage.md).

## Memory looks higher than 12 MB

About 45 MB after the first capture is expected. The extra memory is system graphics and font caches, and it stays flat. If it keeps growing across captures, run `leaks $(pgrep -x Photon)` and see [Development](development.md#memory-checks).

## Reset everything

1. Quit Photon.
2. Delete its settings:

```bash
defaults delete top.piedol.photon
```

3. Relaunch it.
