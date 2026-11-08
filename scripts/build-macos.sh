#!/bin/sh
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$PROJECT_ROOT"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"
sh scripts/cargo.sh build --release --locked
APP="$PROJECT_ROOT/dist/TorrStream Lite.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$PROJECT_ROOT/.build/swift-cache"
cp target/release/torrstream-lite "$APP/Contents/Resources/torrstream-lite"
xcrun swiftc -O -target "$(uname -m)-apple-macosx$MACOSX_DEPLOYMENT_TARGET" -module-cache-path "$PROJECT_ROOT/.build/swift-cache" macos/App.swift -o "$APP/Contents/MacOS/TorrStreamLite" -framework Cocoa -framework WebKit
cp macos/Info.plist "$APP/Contents/Info.plist"
mkdir -p "$APP/Contents/Resources/licenses"
cp LICENSE THIRD_PARTY_NOTICES.txt "$APP/Contents/Resources/"
cp licenses/Apache-2.0.txt "$APP/Contents/Resources/licenses/"
codesign --force --sign - "$APP/Contents/Resources/torrstream-lite"
codesign --force --sign - "$APP"
printf 'Built %s\n' "$APP"
