#!/bin/zsh
# Package an app as a disk image that opens to a window with the app, an arrow and the
# Applications folder, so it's installed by dragging. Signs the image, and with --notarize also
# notarizes and staples it. The app inside should already be signed (and notarized for a release).
# Shared with piedol's other Mac apps and kept the same in each.
#
#   scripts/make-dmg.sh <App.app> <out.dmg> <accent #rrggbb> <signing identity> [--notarize <keychain profile>]
#
# Needs dmgbuild (https://github.com/dmgbuild/dmgbuild), which lays out the window without
# scripting Finder: `python3 -m venv ~/.local/share/dmgbuild-venv &&
# ~/.local/share/dmgbuild-venv/bin/pip install dmgbuild`, or set $DMGBUILD to its path.
set -euo pipefail

(( $# >= 4 )) || { echo "usage: scripts/make-dmg.sh <App.app> <out.dmg> <#rrggbb> <identity> [--notarize <profile>]"; exit 2; }
APP=${1:A}; OUT=${2:A}; ACCENT=$3; IDENTITY=$4
cd "${0:A:h}/.."
PROFILE=""
if (( $# >= 5 )); then
  [[ $5 == --notarize && -n ${6:-} ]] || { echo "unknown option $5"; exit 2; }
  PROFILE=$6
fi
[[ -d $APP ]] || { echo "No app at $APP"; exit 1; }

DMGBUILD=${DMGBUILD:-$(command -v dmgbuild || echo "$HOME/.local/share/dmgbuild-venv/bin/dmgbuild")}
[[ -x $DMGBUILD ]] || { echo "dmgbuild isn't installed. See the comment at the top of scripts/make-dmg.sh."; exit 1; }

NAME=$(basename "$APP" .app)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

swift scripts/dmg-background.swift "$ACCENT" "$WORK/bg.png" "$WORK/bg@2x.png"
tiffutil -cathidpicheck "$WORK/bg.png" "$WORK/bg@2x.png" -out "$WORK/background.tiff" >/dev/null

rm -f "$OUT"
"$DMGBUILD" -s scripts/dmg-settings.py \
  -D app="$APP" -D background="$WORK/background.tiff" -D icon="$APP/Contents/Resources/AppIcon.icns" \
  "$NAME" "$OUT" > /dev/null 2> >(grep -v "is deprecated" >&2)

codesign --force --sign "$IDENTITY" --timestamp "$OUT"
codesign --verify --strict "$OUT"

if [[ -n $PROFILE ]]; then
  xcrun notarytool submit "$OUT" --keychain-profile "$PROFILE" --wait 2>&1 | tee "$WORK/notarize.log"
  grep -q "status: Accepted" "$WORK/notarize.log" || { echo "Notarization of $OUT failed."; exit 1; }
  xcrun stapler staple "$OUT"
  xcrun stapler validate "$OUT"
fi
echo "Disk image: $OUT ($(du -h "$OUT" | cut -f1))"
