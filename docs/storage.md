# Storage: where bytes live

Photon keeps no database and no cache of your captures. Every byte lives in one of these places.

| Data | Where | Format | Lifetime | Deleted by |
|---|---|---|---|---|
| Frozen screen frames | Process memory (and a GPU copy) | CGImage, display colour space | Only while the overlay is open | Closing the overlay (any action or Esc) |
| Markup | Process memory | Vector shapes | Only while the overlay is open | Same |
| Copied screenshot | System clipboard | PNG, 144 dpi on Retina | Until something else is copied | macOS |
| Auto-saved screenshot | `shotDir` (default `~/Pictures/Photon`) | PNG | Permanent | You |
| ⌘S screenshot | Folder you choose (remembered as `lastSaveDir`) | PNG or JPEG | Permanent | You |
| Pinned capture | Process memory | CGImage | Until the pin is closed or Photon quits | Closing the pin |
| Recording in progress | `$TMPDIR/photon-recordings/segment-<ms>.mp4` (one per pause segment) | MP4 | Until the recording stops | Moved or joined at stop. Any leftovers are deleted the next time Photon launches. |
| Joined recording (paused recordings) | `$TMPDIR/photon-recordings/joined-<pid>.mp4` | MP4 | Moments | Moved into the folder |
| Finished recording | `recDir` (default `~/Movies/Photon`) | MP4 (H.264 or HEVC, AAC) | Permanent | You |
| Second copy | `recAlsoDir` (when "Also copy" is on) | MP4 | Permanent | You |
| Trim | Replaces the recording in place (via `<name>.trimming.mp4` next to it) | MP4 | — | The original is overwritten |
| GIF | Next to the recording, same name with `.gif` | Looping GIF, 12 fps, ≤ 960 px, ≤ 60 s | Permanent | You |
| Settings | `~/Library/Preferences/top.piedol.photon.plist` | plist | Permanent | `defaults delete top.piedol.photon` |
| Launch at Login | macOS Background Task Management (SMAppService) | — | Until turned off | Settings › General or `--login-item off` |

## Cloud folders

"Sync to the cloud" means choosing a folder that a cloud provider's own Mac app already syncs. Photon finds them under `~/Library/CloudStorage`:

| Provider | Folder pattern | Default target |
|---|---|---|
| OneDrive | `OneDrive-<account>` | `<root>/Photon` |
| Google Drive | `GoogleDrive-<account>` | `<root>/My Drive/Photon` |
| Dropbox | `Dropbox` or `Dropbox-<account>` | `<root>/Photon` |
| Box | `Box` or `Box-<account>` | `<root>/Photon` |
| iCloud Drive | `~/Library/Mobile Documents/com~apple~CloudDocs` | `<root>/Photon` |

Folders ending in a bracketed suffix, like `OneDrive-Personal (11-09-2026 4:05 PM)`, are usually leftovers from a reinstall. They're listed last and marked "(old)".

**Upload** is done entirely by the provider's app. Photon writes the file and is finished with it. Upload speed, conflicts and quotas are the provider's.

**Fallback:** before writing, Photon checks that the provider's root folder still exists. It never creates a fake provider folder. If the root is missing, the file goes to the local default (`~/Pictures/Photon` or `~/Movies/Photon`), the HUD says "cloud folder missing", and Settings shows an orange note.

## Write safety

- Screenshots are written to a hidden temp file in the destination folder and then renamed, so a partial PNG never appears.
- Recordings are written in the temp folder and moved into place when complete. Across volumes, they're copied and the temp file deleted.
- Names never overwrite an existing file (see [Naming templates](configuration.md#naming-templates)).
