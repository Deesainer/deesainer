#!/usr/bin/env bash
# Builds a tar.gz and (when dpkg-deb exists) a .deb for Linux into dist/.
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
ARCH=$(uname -m)
cargo build --release -p deesainer -p deesainer-cli -p deesainer-relay -p deesainer-teamserver
TARGET_DIR=${CARGO_TARGET_DIR:-target}
STAGE=dist/stage
rm -rf dist && mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/applications" "$STAGE/usr/share/icons/hicolor/256x256/apps"
for bin in deesainer deesainer-cli deesainer-relay deesainer-server; do
  install -m755 "$TARGET_DIR/release/$bin" "$STAGE/usr/bin/"
done
install -m644 packaging/linux/deesainer.desktop "$STAGE/usr/share/applications/"
install -m644 assets/app-icon/png/256.png "$STAGE/usr/share/icons/hicolor/256x256/apps/deesainer.png"
tar -C "$STAGE" -czf "dist/deesainer-$VERSION-linux-$ARCH.tar.gz" usr
if command -v dpkg-deb >/dev/null; then
  mkdir -p "$STAGE/DEBIAN"
  DEB_ARCH=$([[ "$ARCH" == "x86_64" ]] && echo amd64 || echo arm64)
  # Debian sorts 1.1.0~beta.1 before 1.1.0.
  DEB_VERSION=$(printf '%s' "$VERSION" | tr '-' '~')
  cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: deesainer
Version: $DEB_VERSION
Architecture: $DEB_ARCH
Maintainer: Deesainer contributors
# winit needs libxkbcommon-x11 at runtime (the app panics without it), gtk for
# the file dialogs, and a GL implementation for the canvas.
Depends: libxkbcommon-x11-0, libgtk-3-0, libgl1, libegl1
Description: Open-source interface design tool
 Clean-room design tool with an infinite canvas, components, auto layout,
 prototyping, multiplayer and an MCP server for agents.
CONTROL
  dpkg-deb --build --root-owner-group "$STAGE" "dist/deesainer_${DEB_VERSION}_${DEB_ARCH}.deb"
fi
rm -rf "$STAGE"
echo "dist: $(ls dist)"
