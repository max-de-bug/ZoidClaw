# ── Build stage ──────────────────────────────────────────────────────
FROM rust:1.77-bookworm AS builder

WORKDIR /build
COPY . .

# Build in release mode with all features.
RUN cargo build --release --workspace

# ── Runtime stage ────────────────────────────────────────────────────
FROM debian:bookworm-slim

# TLS certificates for HTTPS API calls.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/crabbybot /usr/local/bin/crabbybot

# Default configuration mount point.
VOLUME ["/root/.crabbybot"]

ENTRYPOINT ["crabbybot"]
CMD ["bot"]
