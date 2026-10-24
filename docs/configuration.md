# Configuration

Everything is set in **Settings** (menu bar › Settings…, or `open photon://settings`). Changes apply and save straight away; there's no Save button.

## Settings window

### General

- **Shortcuts:** click a shortcut, then press the new keys.
  - A shortcut needs ⌘, ⌃ or ⌥.
  - Esc cancels.
  - While you're choosing, the existing shortcuts are paused.
  - If another app already owns a combination, the note under the buttons says so.
- **Launch Photon at login**

### Screenshots

- **Also save a PNG automatically:** off by default, so screenshots go to the clipboard only.
- **Save to:** the default folder, any detected cloud folder, or *Other Folder…*.
- **File names:** see [Naming templates](#naming-templates).

### Recordings

- **Save to:** the default folder, any detected cloud folder, or *Other Folder…*.
- **Also copy each recording to:** a second folder, typically a cloud folder.
- **File names:** see [Naming templates](#naming-templates).
- **Format:** H.264 (most compatible) or HEVC (smaller files).
- **Frame rate:** 30 or 60 fps.
- **Record system audio** and **Record microphone**, with a microphone picker.
- **Webcam bubble**, with a camera picker.
- **Show the pointer**, **Highlight clicks**, **3-second countdown**.

If a chosen cloud folder disappears (for example because you signed out of the provider), Settings shows an orange note. Files then go to the default local folder until it's back. See [Storage](storage.md).

## Stored preferences

Settings live in the standard macOS preferences for the domain `top.piedol.photon`, in `~/Library/Preferences/top.piedol.photon.plist`. You can inspect them with `defaults read top.piedol.photon`.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `shotHotkey` | int | ⇧⌘9 | Screenshot shortcut, encoded as `(modifierFlags << 16) \| keyCode` |
| `recordHotkey` | int | ⇧⌘0 | Record shortcut, same encoding |
| `shotAutosave` | bool | `false` | Also write a PNG when copying |
| `shotDir` | string | `~/Pictures/Photon` | Screenshot folder |
| `lastSaveDir` | string | — | Last folder chosen in the ⌘S save panel |
| `recDir` | string | `~/Movies/Photon` | Recording folder |
| `recAlsoCopy` | bool | `false` | Copy each recording to a second folder |
| `recAlsoDir` | string | — | That second folder |
| `recCodec` | string | `h264` | `h264` or `hevc` |
| `recFps60` | int | `60` | `30` or `60` (replaces the old `recFps` key) |
| `recSystemAudio` | bool | `true` | Capture system audio (Photon's own sounds are excluded) |
| `recMic` | bool | `false` | Capture the microphone |
| `recMicDevice` | string | system default | Microphone unique ID |
| `recWebcam` | bool | `false` | Show the webcam bubble |
| `recWebcamDevice` | string | system default | Camera unique ID |
| `recCountdown` | bool | `true` | 3-second countdown |
| `recCursor` | bool | `true` | Show the pointer |
| `recClickRings` | bool | `true` | Highlight clicks |
| `toolColor` | int | `0` (red) | Last swatch, 0–7 |
| `stroke` | int | `1` (medium) | Last stroke size, 0–2 |
| `shotNameTemplate`, `shotNameDate`, `shotNameTime`, `shotNameCase`, `shotNameSep` | strings | see below | Screenshot naming rule |
| `recNameTemplate`, `recNameDate`, `recNameTime`, `recNameCase`, `recNameSep` | strings | see below | Recording naming rule |
| `shotCounter`, `recCounter` | int | `1` | Next `{counter}` value |

Unknown or invalid values fall back to the defaults. To reset everything, quit Photon, then:

```bash
defaults delete top.piedol.photon
```

## Naming templates

Screenshots and recordings each have their own template.

| Token | Becomes |
|---|---|
| `{type}` | `Screenshot` or `Recording` |
| `{date}` | Date, in the chosen format |
| `{time}` | Time, in the chosen format |
| `{app}` | App in front when you pressed the shortcut. For window recordings, the window's app. |
| `{size}` | Pixel size, e.g. `1440x900` |
| `{display}` | Display name |
| `{counter}` | Running number, zero-padded to 3 digits (`007`) |

Tokens are case-insensitive. Unknown `{words}` stay as plain text. Anything else you type is kept.

| Option | Choices (stored key) |
|---|---|
| Date | `2026-10-24` (`iso`), `Oct 24 2026` (`month_day_year`), `24-10-2026` (`day_month_year`), `20261024` (`compact`) |
| Time | `09.41.12` (`dots`), `09-41-12` (`dashes`), `0941` (`compact`), `9.41 AM` (`twelve_hour`) |
| Case | As typed (`as_typed`), Title Case (`title`), lowercase (`lower`), UPPERCASE (`upper`) |
| Separator | Space (`space`), Dash (`dash`), Underscore (`underscore`). This also replaces spaces inside token values. |

The defaults are:

| | Template | Example |
|---|---|---|
| Screenshots | `{type} {date} at {time}` | `Screenshot 2026-10-24 at 09.41.12.png` |
| Recordings | `Photon {type} {date} at {time}` | `Photon Recording 2026-10-24 at 09.41.12.mp4` |

Names are always made safe:
- `/`, `:` and `\` become `-`.
- Control characters and leading dots are removed.
- Repeated separators collapse.
- Names are capped at 200 bytes.
- An empty result falls back to the default template.

If the name is taken, ` (2)`, `-2` or `_2` is added (matching the separator). With `{counter}`, the counter advances instead.

The Settings window shows a live preview. The ⌘S save panel is pre-filled from the screenshot template.

## Environment variables

None are needed for normal use.

| Variable | Used by | Effect |
|---|---|---|
| `PHOTON_LOG` | app | Print debug logs to stderr (always on in debug builds) |
| `PHOTON_DEV` | app | Enables the `photon://selftest*` URLs (see [Development](development.md)) |
| `PHOTON_SHOTS` | self-tests | Folder for self-test screenshots (default `/tmp/photon-shots`) |
| `PHOTON_SIGN_IDENTITY` | `scripts/bundle.sh` | Signing identity override |
