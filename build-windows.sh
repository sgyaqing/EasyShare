#!/bin/sh
# Cross-compile a Windows x86_64 executable inside a Docker container.
# Produces easyshare-windows-x86_64.exe (windows-gnu, runs on any 64-bit
# Windows without extra runtime installs).
#
# Usage: ./build-windows.sh
set -e
cd "$(dirname "$0")"

IMAGE="easyshare-windows-builder"
TRIPLE="x86_64-pc-windows-gnu"

if ! docker image inspect "$IMAGE" > /dev/null 2>&1; then
    echo "Building builder image..."
    docker build --platform linux/amd64 -f Dockerfile.windows -t "$IMAGE" .
fi

mkdir -p target/windows cargo-registry

echo "Compiling for $TRIPLE..."
docker run --rm --platform linux/amd64 \
    -v "$PWD":/src -w /src \
    -v "$PWD/target/windows":/src/target \
    -v "$PWD/cargo-registry":/usr/local/cargo/registry \
    -v "$PWD/docker-cargo-config.toml":/usr/local/cargo/config.toml:ro \
    "$IMAGE" \
    cargo build --release --target "$TRIPLE"

OUT="easyshare-v$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)-windows-x86_64.exe"
cp "target/windows/$TRIPLE/release/easyshare.exe" "$OUT"
echo "Built: $OUT"
ls -lh "$OUT"
