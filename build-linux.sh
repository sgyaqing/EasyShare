#!/bin/sh
# Build a static Linux binary inside a Docker container.
# The result is fully static (musl), so it runs on any Linux regardless
# of its libc version. Deploy = copy easyshare-linux-<arch> + nothing else.
#
# Usage: ./build-linux.sh [amd64|arm64]   (default: amd64)
set -e
cd "$(dirname "$0")"

ARCH="${1:-amd64}"
case "$ARCH" in
    amd64) PLATFORM="linux/amd64";  TRIPLE="x86_64-unknown-linux-musl" ;;
    arm64) PLATFORM="linux/arm64";  TRIPLE="aarch64-unknown-linux-musl" ;;
    *) echo "usage: ./build-linux.sh [amd64|arm64]"; exit 1 ;;
esac

IMAGE="easyshare-linux-builder-$ARCH"

if ! docker image inspect "$IMAGE" > /dev/null 2>&1; then
    echo "Building builder image for $PLATFORM..."
    docker build --platform "$PLATFORM" -f Dockerfile.linux -t "$IMAGE" .
fi

# Persistent volumes for cargo registry and build cache, so rebuilds are fast.
mkdir -p target/linux cargo-registry

echo "Compiling for $TRIPLE ($PLATFORM)..."
docker run --rm --platform "$PLATFORM" \
    -v "$PWD":/src -w /src \
    -v "$PWD/target/linux":/src/target \
    -v "$PWD/cargo-registry":/usr/local/cargo/registry \
    -v "$PWD/docker-cargo-config.toml":/usr/local/cargo/config.toml:ro \
    "$IMAGE" \
    cargo build --release --target "$TRIPLE"

OUT="easyshare-v$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)-linux-$ARCH"
cp "target/linux/$TRIPLE/release/easyshare" "$OUT"
echo "Built: $OUT"
ls -lh "$OUT"
