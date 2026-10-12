#!/usr/bin/env bash
# Builds Deesainer.app (universal) and a DMG into dist/.
# Signing and notarization run only when their secrets are set; otherwise the
# bundle is signed ad hoc and the script prints a warning (fine for local tests).
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
BUNDLE_ID=${DEESAINER_BUNDLE_ID:-dev.deesainer.app}
export MACOSX_DEPLOYMENT_TARGET=11.0
# The main executable last: signing it checks the helpers next to it.
BINS="deesainer-cli deesainer-relay deesainer-server deesainer"
TARGET_DIR=${CARGO_TARGET_DIR:-target}
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$target" >/dev/null
  cargo build --release --target "$target" -p deesainer -p deesainer-cli -p deesainer-relay -p deesainer-teamserver
done
APP=dist/Deesainer.app
rm -rf dist && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
for bin in $BINS; do
  lipo -create -output "$APP/Contents/MacOS/$bin" \
    "$TARGET_DIR/aarch64-apple-darwin/release/$bin" "$TARGET_DIR/x86_64-apple-darwin/release/$bin"
done
# macOS bundle versions are numeric: 1.1.0-beta.1 shows as 1.1.0 there.
sed -e "s/NUMERIC/${VERSION%%-*}/g" -e "s/BUNDLE_ID/$BUNDLE_ID/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp assets/app-icon/deesainer.icns "$APP/Contents/Resources/"
if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  # Inside-out: executables first, then the bundle, hardened runtime, timestamp.
  for bin in $BINS; do
    codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$APP/Contents/MacOS/$bin"
  done
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$APP"
else
  echo "::warning::MACOS_SIGNING_IDENTITY not set; signing ad hoc (Gatekeeper will warn)"
  for bin in $BINS; do
    codesign --force --sign - "$APP/Contents/MacOS/$bin"
  done
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP" || true
DMG="dist/Deesainer-$VERSION-macos-universal.dmg"
mkdir -p dist/dmg && cp -R "$APP" dist/dmg/ && ln -s /Applications dist/dmg/Applications
hdiutil create -volname Deesainer -srcfolder dist/dmg -ov -format UDZO "$DMG"
rm -rf dist/dmg
if [[ -n "${MACOS_NOTARY_PROFILE:-}" && -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$DMG"
  xcrun notarytool submit "$DMG" --keychain-profile "$MACOS_NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
  spctl -a -vvv -t install "$DMG" || true
else
  echo "::warning::notarization skipped (no MACOS_NOTARY_PROFILE)"
fi
(cd dist && zip -qr "Deesainer-$VERSION-macos-cli.zip" Deesainer.app/Contents/MacOS/deesainer-cli Deesainer.app/Contents/MacOS/deesainer-relay Deesainer.app/Contents/MacOS/deesainer-server)
echo "dist: $(ls dist)"
