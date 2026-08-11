#!/bin/sh
# Build macOS release binaries natively (no container needed).
# Usage: ./build-mac.sh [arm64|x86_64|universal]   (default: universal)
set -e
cd "$(dirname "$0")"

ARCH="${1:-universal}"
VER=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)

build() {
    triple="$1"
    if ! rustup target list --installed | grep -q "^$triple$"; then
        echo "Adding Rust target $triple..."
        rustup target add "$triple"
    fi
    echo "Compiling for $triple..."
    cargo build --release --target "$triple"
}

# Assemble EasyShare.app: double-click opens Terminal running the server
# (banner shows the LAN IP) and opens the browser at the chat page.
pack_app() {
    APP="EasyShare.app"
    echo "Assembling $APP..."
    rm -rf "$APP"
    mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

    cp target/aarch64-apple-darwin/release/easyshare "$APP/Contents/MacOS/easyshare-arm64"
    cp target/x86_64-apple-darwin/release/easyshare "$APP/Contents/MacOS/easyshare-x86_64"
    lipo -create -output "$APP/Contents/MacOS/easyshare-bin" \
        "$APP/Contents/MacOS/easyshare-arm64" \
        "$APP/Contents/MacOS/easyshare-x86_64"
    rm "$APP/Contents/MacOS/easyshare-arm64" "$APP/Contents/MacOS/easyshare-x86_64"

    cat > "$APP/Contents/MacOS/EasyShare" <<'LAUNCHER'
#!/bin/sh
DIR="$(cd "$(dirname "$0")" && pwd)"
(sleep 2 && open "http://localhost:8972") &
exec open -a Terminal "$DIR/easyshare-bin"
LAUNCHER
    chmod +x "$APP/Contents/MacOS/EasyShare"

    cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>EasyShare</string>
    <key>CFBundleIdentifier</key><string>com.easyshare.app</string>
    <key>CFBundleName</key><string>EasyShare</string>
    <key>CFBundleShortVersionString</key><string>${VER}</string>
    <key>CFBundleVersion</key><string>${VER}</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
</dict>
</plist>
PLIST

    # Build icon.icns from the iconset.
    ICONSET=$(mktemp -d)/icon.iconset
    mkdir -p "$ICONSET"
    cp assets/icon-16.png   "$ICONSET/icon_16x16.png"
    cp assets/icon-32.png   "$ICONSET/icon_16x16@2x.png"
    cp assets/icon-32.png   "$ICONSET/icon_32x32.png"
    cp assets/icon-64.png   "$ICONSET/icon_32x32@2x.png"
    cp assets/icon-128.png  "$ICONSET/icon_128x128.png"
    cp assets/icon-256.png  "$ICONSET/icon_128x128@2x.png"
    cp assets/icon-256.png  "$ICONSET/icon_256x256.png"
    cp assets/icon-512.png  "$ICONSET/icon_256x256@2x.png"
    cp assets/icon-512.png  "$ICONSET/icon_512x512.png"
    cp assets/icon-1024.png "$ICONSET/icon_512x512@2x.png"
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
    echo "Built: $APP"
}

case "$ARCH" in
    arm64)
        build aarch64-apple-darwin
        cp target/aarch64-apple-darwin/release/easyshare "easyshare-v${VER}-mac-arm64"
        ls -lh "easyshare-v${VER}-mac-arm64"
        ;;
    x86_64)
        build x86_64-apple-darwin
        cp target/x86_64-apple-darwin/release/easyshare "easyshare-v${VER}-mac-x86_64"
        ls -lh "easyshare-v${VER}-mac-x86_64"
        ;;
    universal)
        build aarch64-apple-darwin
        build x86_64-apple-darwin
        OUT="easyshare-v${VER}-mac-universal"
        lipo -create -output "$OUT" \
            target/aarch64-apple-darwin/release/easyshare \
            target/x86_64-apple-darwin/release/easyshare
        echo "Built: $OUT"
        lipo -info "$OUT"
        ls -lh "$OUT"
        pack_app
        ;;
    *)
        echo "usage: ./build-mac.sh [arm64|x86_64|universal]"
        exit 1
        ;;
esac
