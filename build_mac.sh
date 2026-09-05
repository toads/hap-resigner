#!/bin/bash
# Build the self-contained Rust/egui HAP Resigner app for macOS arm64.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN="$ROOT/target/release/hap-resigner"
CLI="$ROOT/target/release/hap-resigner-cli"
DIST="$ROOT/dist"
APP="$DIST/HAP Resigner.app"
MACOS="$APP/Contents/MacOS"

cd "$ROOT"
PACKAGE_ID="$(cargo pkgid)"
VERSION="${PACKAGE_ID##*@}"
if [[ "$VERSION" == "$PACKAGE_ID" || -z "$VERSION" ]]; then
  echo "Unable to determine package version from Cargo package ID: $PACKAGE_ID" >&2
  exit 1
fi
ARCHIVE_NAME="HAP-Resigner-v${VERSION}-macos-arm64.zip"
ARCHIVE="$DIST/$ARCHIVE_NAME"
CHECKSUM="$ARCHIVE.sha256"

cargo build --locked --release --features app
"$CLI" --selftest

rm -rf "$APP" "$ARCHIVE" "$CHECKSUM" \
  "$DIST/HAP Resigner.app.zip" "$DIST/HAP Resigner.app.zip.sha256"
mkdir -p "$MACOS"
cp "$BIN" "$MACOS/hap-resigner"
chmod 755 "$MACOS/hap-resigner"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>HAP Resigner</string>
  <key>CFBundleExecutable</key><string>hap-resigner</string>
  <key>CFBundleIdentifier</key><string>com.ohcodesec.hap-resigner</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>HAP Resigner</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
ditto -c -k --keepParent "$APP" "$ARCHIVE"
(
  cd "$DIST"
  shasum -a 256 "$ARCHIVE_NAME" > "$ARCHIVE_NAME.sha256"
)

echo "BUILD_OK: $APP"
echo "ARCHIVE_OK: $ARCHIVE"
echo "CHECKSUM_OK: $CHECKSUM"
du -sh "$APP" "$ARCHIVE"
