#!/bin/sh
# Build all platform release artifacts and package them for GitHub Release.
# Everything is built in release mode; output lands in release/v<version>/,
# containing both the raw executables and the packaged archives.
#
# Usage: ./release.sh
set -e
cd "$(dirname "$0")"

VER=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
REL="release/v$VER"

echo "=== EasyShare release build: v$VER ==="
rm -rf "$REL"
mkdir -p "$REL"

# --- 1. Build all platforms (all release mode) ---
./build-mac.sh universal
./build-linux.sh amd64
./build-linux.sh arm64
./build-windows.sh

# --- 2. Move raw executables into the release dir (keep project root clean) ---
# macOS ships only as EasyShare.app; the raw universal binary is just an
# intermediate for app assembly, so remove it after packaging.
mv "easyshare-v$VER-linux-amd64"        "$REL/"
mv "easyshare-v$VER-linux-arm64"        "$REL/"
mv "easyshare-v$VER-windows-x86_64.exe" "$REL/"

# --- 3. Package: tar.gz for Linux (binary inside is plain "easyshare"),
#        zip for Windows ("easyshare.exe"), zip of the app bundle for Mac ---
pack_tar() {
    src="$1"; out="$2"
    tmp=$(mktemp -d)
    cp "$src" "$tmp/easyshare"
    tar -czf "$REL/$out" -C "$tmp" easyshare
    rm -rf "$tmp"
}

pack_tar "$REL/easyshare-v$VER-linux-amd64" "easyshare-v$VER-linux-amd64.tar.gz"
pack_tar "$REL/easyshare-v$VER-linux-arm64" "easyshare-v$VER-linux-arm64.tar.gz"

tmp=$(mktemp -d)
cp "$REL/easyshare-v$VER-windows-x86_64.exe" "$tmp/easyshare.exe"
(cd "$tmp" && zip -q -X "$OLDPWD/$REL/easyshare-v$VER-windows-x86_64.zip" easyshare.exe)
rm -rf "$tmp"

ditto EasyShare.app "$REL/EasyShare.app"
ditto -c -k --sequesterRsrc --keepParent EasyShare.app "$REL/EasyShare-v$VER-mac.zip"
rm -f "easyshare-v$VER-mac-universal"

# --- 4. Checksums (packages only — those are what gets uploaded) ---
(cd "$REL" && find . -maxdepth 1 -type f \( -name "*.zip" -o -name "*.tar.gz" \) -exec shasum -a 256 {} + | sed 's| \./| |' > sha256sums.txt)

echo "=== Release artifacts in $REL ==="
ls -lh "$REL"
