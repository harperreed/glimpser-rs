# ABOUTME: Lean Dockerfile for Rust backend with minimal disk usage
# ABOUTME: Multi-stage build that only copies necessary files and cleans up after build

FROM rust:1.83 AS builder

# Install Chromium and build dependencies
RUN apt-get update && apt-get install -y \
    chromium \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install nightly Rust for edition2024 support
RUN rustup toolchain install nightly && rustup default nightly

WORKDIR /app

# Copy all source code and build
COPY . .

# Build the application (single build to avoid complexity)
RUN cargo build --release --bin glimpser

# Final runtime stage
FROM debian:bookworm-slim

# Install only runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    chromium \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false -m -d /app app

WORKDIR /app

# Copy only the binary
COPY --from=builder --chown=app:app /app/target/release/glimpser ./glimpser
COPY --chown=app:app gl_db/migrations ./gl_db/migrations

USER app
EXPOSE 3000

ENTRYPOINT ["./glimpser"]
