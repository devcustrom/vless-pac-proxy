#!/bin/bash
# Сборка WireproxyControl.app, zip и dmg на macOS без GitHub Actions.
# Требования: Xcode Command Line Tools, Node.js, Rust.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

missing=()
for cmd in node npm cargo sips iconutil codesign; do
  command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "Не найдены: ${missing[*]}"
  echo "Установите Xcode Command Line Tools: xcode-select --install"
  echo "Node.js: https://nodejs.org или brew install node"
  echo "Rust:    https://rustup.rs"
  exit 1
fi

VERSION=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
APP="WireproxyControl.app"
DMG="wireproxy-control-macos-universal.dmg"
ZIP="wireproxy-control-macos-universal.zip"
BIN="wireproxy-control-macos-universal"

echo "==> фронтенд"
npm --prefix web ci
npm --prefix web run build

echo "==> Rust"
# Явный deployment target обязателен: иначе Rust проставит минимальную macOS
# сборочной машины (например 26.0), и приложение не запустится на более
# старой системе с сообщением «Use Software Update in System Settings».
if rustup target list --installed | grep -qx "aarch64-apple-darwin" &&
   rustup target list --installed | grep -qx "x86_64-apple-darwin"; then
  MACOSX_DEPLOYMENT_TARGET=11.0 \
    cargo build --release --target aarch64-apple-darwin
  MACOSX_DEPLOYMENT_TARGET=10.13 \
    cargo build --release --target x86_64-apple-darwin
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/wireproxy-control \
    target/x86_64-apple-darwin/release/wireproxy-control
else
  case "$(uname -m)" in
    arm64) MIN_MACOS="11.0" ;;
    *)     MIN_MACOS="10.13" ;;
  esac
  MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS" cargo build --release
  cp target/release/wireproxy-control "$BIN"
fi
chmod +x "$BIN"
codesign --force --sign - "$BIN"

echo "==> иконка"
rm -rf AppIcon.iconset AppIcon.icns
mkdir -p AppIcon.iconset
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/AppIcon.png \
    --out "AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/AppIcon.png \
    --out "AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns AppIcon.iconset -o AppIcon.icns

echo "==> .app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/wireproxy-control"
cp AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.devcustrom.wireproxy-control</string>
  <key>CFBundleName</key><string>wireproxy-control</string>
  <key>CFBundleDisplayName</key><string>WireproxyControl</string>
  <key>CFBundleExecutable</key><string>wireproxy-control</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>10.13</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
xattr -cr "$APP" || true
codesign --force --deep --sign - "$APP"

echo "==> zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "==> dmg"
STAGE="dmg-stage"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
if command -v create-dmg >/dev/null 2>&1; then
  create-dmg \
    --volname "wireproxy-control" \
    --volicon AppIcon.icns \
    --background assets/dmg-background.png \
    --window-pos 200 120 \
    --window-size 640 420 \
    --icon-size 128 \
    --icon "WireproxyControl.app" 170 210 \
    --hide-extension "WireproxyControl.app" \
    --app-drop-link 470 210 \
    "$DMG" "$STAGE" || true
fi
if [ ! -f "$DMG" ]; then
  ln -sfn /Applications "$STAGE/Applications"
  hdiutil create -volname "wireproxy-control" -srcfolder "$STAGE" \
    -ov -format UDZO "$DMG"
fi

echo
echo "Готово:"
echo "  $ROOT/$APP"
echo "  $ROOT/$ZIP"
echo "  $ROOT/$DMG"
