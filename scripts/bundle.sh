#!/bin/zsh
# Build Photon.app for Apple silicon, sign it, and optionally install or notarize.
#   scripts/bundle.sh             build + sign into dist/Photon.app
#   scripts/bundle.sh --install   also copy to /Applications and relaunch
#   scripts/bundle.sh --notarize  also notarize and zip for release (keychain profile $NOTARY_PROFILE, default piedol-notary)
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
# Sign with $PHOTON_SIGN_IDENTITY, or else the first Developer ID Application certificate
# in the keychain. Without either, the app gets an ad-hoc signature.
# Notarization needs a secure timestamp; everyday builds skip it to stay fast and work offline.
TS=--timestamp=none; (( NOTARIZE )) && TS=--timestamp
IDENTITY=${PHOTON_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)}

# Keep build-machine paths (home folder, checkout) out of the binary's panic messages.
# The encoded form separates flags with 0x1f, so paths with spaces work.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME/.cargo=/cargo"$'\x1f'"--remap-path-prefix=$PWD=/photon"
cargo build --release --target aarch64-apple-darwin

APP=dist/Photon.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/aarch64-apple-darwin/release/Photon "$APP/Contents/MacOS/Photon"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" resources/Info.plist > "$APP/Contents/Info.plist"
[[ -f assets/AppIcon.icns ]] && cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

if [[ -n $IDENTITY ]] && security find-identity -v -p codesigning | grep -qF -- "\"$IDENTITY\""; then
  codesign --force --options runtime "$TS" \
    --entitlements resources/Photon.entitlements --sign "$IDENTITY" "$APP"
else
  echo "warning: signing identity not found, using ad-hoc signature (permissions reset on every build)"
  codesign --force --options runtime --entitlements resources/Photon.entitlements --sign - "$APP"
fi
codesign --verify --strict "$APP"

if (( NOTARIZE )); then
  # Notarization needs a keychain profile made once with:
  #   xcrun notarytool store-credentials <profile> --apple-id <Apple ID> --team-id <team ID>
  PROFILE=${NOTARY_PROFILE:-piedol-notary}
  ditto -c -k --keepParent "$APP" dist/Photon-notarize.zip
  xcrun notarytool submit dist/Photon-notarize.zip --keychain-profile "$PROFILE" --wait
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  rm -f dist/Photon-notarize.zip
  ditto -c -k --keepParent "$APP" dist/Photon-macOS-arm64.zip
  echo "Notarized: dist/Photon-macOS-arm64.zip"
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
