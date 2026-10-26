#!/bin/zsh
# Build Photon.app for Apple silicon, sign it, and optionally install or notarize.
#   scripts/bundle.sh             build + sign into dist/Photon.app
#   scripts/bundle.sh --install   also copy to /Applications and relaunch
#   scripts/bundle.sh --notarize  also notarize (needs a notarytool keychain profile named "photon")
set -euo pipefail
cd "${0:A:h}/.."

INSTALL=0; NOTARIZE=0
for a in "$@"; do
  case $a in
    --install) INSTALL=1 ;;
    --notarize) NOTARIZE=1 ;;
    *) echo "unknown option $a"; exit 2 ;;
  esac
done

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 1)
IDENTITY=${PHOTON_SIGN_IDENTITY:-"Developer ID Application: Your Name (TEAMID)"}

cargo build --release --target aarch64-apple-darwin

APP=dist/Photon.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/aarch64-apple-darwin/release/Photon "$APP/Contents/MacOS/Photon"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" resources/Info.plist > "$APP/Contents/Info.plist"
[[ -f assets/AppIcon.icns ]] && cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

if security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  codesign --force --options runtime --timestamp=none \
    --entitlements resources/Photon.entitlements --sign "$IDENTITY" "$APP"
else
  echo "warning: signing identity not found, using ad-hoc signature (permissions reset on every build)"
  codesign --force --options runtime --entitlements resources/Photon.entitlements --sign - "$APP"
fi
codesign --verify --strict "$APP"

if (( NOTARIZE )); then
  ditto -c -k --keepParent "$APP" dist/Photon.zip
  xcrun notarytool submit dist/Photon.zip --keychain-profile photon --wait
  xcrun stapler staple "$APP"
fi

echo "Built $APP ($(du -sh "$APP" | cut -f1), binary $(du -h "$APP/Contents/MacOS/Photon" | cut -f1), $(lipo -archs "$APP/Contents/MacOS/Photon"))"

if (( INSTALL )); then
  pkill -x Photon 2>/dev/null || true
  # Relaunching before the old process is gone makes `open` fail with -600.
  for _ in {1..50}; do pgrep -x Photon >/dev/null || break; sleep 0.1; done
  rm -rf /Applications/Photon.app
  ditto "$APP" /Applications/Photon.app
  open /Applications/Photon.app
  echo "Installed to /Applications/Photon.app"
fi
