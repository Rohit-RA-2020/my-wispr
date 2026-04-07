#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="${HOME}/Applications/Wispr.app"
MACOS_DIR="${APP_DIR}/Contents/MacOS"

mkdir -p "${MACOS_DIR}"

cd "${ROOT_DIR}"
cargo build --release --bin wisprd --bin wisprctl

cd "${ROOT_DIR}/apps/WisprMac"
swift build -c release

cat > "${APP_DIR}/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>WisprMacApp</string>
    <key>CFBundleIdentifier</key>
    <string>io.wispr.WisprMac</string>
    <key>CFBundleName</key>
    <string>Wispr</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSMicrophoneUsageDescription</key>
    <string>Wispr needs microphone access to capture speech for dictation.</string>
</dict>
</plist>
PLIST

cp "${ROOT_DIR}/apps/WisprMac/.build/release/WisprMacApp" "${MACOS_DIR}/WisprMacApp"
cp "${ROOT_DIR}/target/release/wisprd" "${MACOS_DIR}/wisprd"
cp "${ROOT_DIR}/target/release/wisprctl" "${MACOS_DIR}/wisprctl"
chmod +x "${MACOS_DIR}/WisprMacApp" "${MACOS_DIR}/wisprd" "${MACOS_DIR}/wisprctl"

echo "Installed ${APP_DIR}"
