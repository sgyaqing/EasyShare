# Build stage: compile the static musl binary.
FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev gcc
# crates.io mirror (Docker Hub / crates.io are unreachable without VPN here).
COPY docker-cargo-config.toml /usr/local/cargo/config.toml
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release && strip target/release/easyshare

# Runtime stage: scratch — the binary is fully static, nothing else needed.
FROM scratch
COPY --from=builder /src/target/release/easyshare /easyshare
EXPOSE 8972
ENTRYPOINT ["/easyshare"]
