# ABOUTME: Production-ready multi-stage Dockerfile for unified HTMX-based Glimpser
# ABOUTME: Optimized for security, size, and performance with proper asset handling

# Stage 1: Build stage with caching optimization
FROM rust:1.83-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    chromium \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install nightly Rust for edition2024 support
RUN rustup toolchain install nightly && rustup default nightly

WORKDIR /app

# First, copy only Cargo files for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY app/Cargo.toml ./app/
COPY gl_capture/Cargo.toml ./gl_capture/
COPY gl_config/Cargo.toml ./gl_config/
COPY gl_core/Cargo.toml ./gl_core/
COPY gl_db/Cargo.toml ./gl_db/
COPY gl_storage/Cargo.toml ./gl_storage/
COPY gl_vision/Cargo.toml ./gl_vision/
COPY gl_web/Cargo.toml ./gl_web/

# Create dummy main.rs files to build dependencies
RUN mkdir -p app/src gl_capture/src gl_config/src gl_core/src gl_db/src gl_storage/src gl_vision/src gl_web/src && \
    echo "fn main() {}" > app/src/main.rs && \
    touch gl_capture/src/lib.rs && \
    touch gl_config/src/lib.rs && \
    touch gl_core/src/lib.rs && \
    touch gl_db/src/lib.rs && \
    touch gl_storage/src/lib.rs && \
    touch gl_vision/src/lib.rs && \
    touch gl_web/src/lib.rs

# Build dependencies only (this layer will be cached)
RUN cargo build --release --bin glimpser

# Now copy the actual source code
COPY . .

# Touch the main.rs to ensure rebuild with actual code
RUN touch app/src/main.rs

# Build the final application
RUN cargo build --release --bin glimpser

# Stage 2: Runtime stage - minimal and secure
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    chromium \
    chromium-driver \
    fonts-liberation \
    libnss3 \
    libatk-bridge2.0-0 \
    libgtk-3-0 \
    libx11-xcb1 \
    libxcomposite1 \
    libxdamage1 \
    libxrandr2 \
    libgbm1 \
    libasound2 \
    libpangocairo-1.0-0 \
    libatk1.0-0 \
    libcups2 \
    libxss1 \
    libappindicator3-1 \
    ffmpeg \
    yt-dlp \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create non-root user with specific UID/GID for consistency
RUN groupadd -g 1001 glimpser && \
    useradd -r -u 1001 -g glimpser -m -d /app -s /bin/false glimpser

WORKDIR /app

# Create necessary directories with proper permissions
RUN mkdir -p \
    /app/data \
    /app/data/storage \
    /app/data/frames \
    /app/data/videos \
    /app/logs \
    /app/templates \
    /app/static \
    && chown -R glimpser:glimpser /app

# Copy application binary
COPY --from=builder --chown=glimpser:glimpser /app/target/release/glimpser ./glimpser

# Copy database migrations
COPY --chown=glimpser:glimpser gl_db/migrations ./gl_db/migrations

# Copy templates and static assets for HTMX frontend
COPY --chown=glimpser:glimpser gl_web/templates ./gl_web/templates
COPY --chown=glimpser:glimpser gl_web/static ./gl_web/static

# Set up Chrome sandbox permissions (required for headless Chrome)
RUN chmod 4755 /usr/lib/chromium/chrome-sandbox

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Switch to non-root user
USER glimpser

# Expose the unified server port
EXPOSE 3000

# Set default environment variables
ENV RUST_LOG=info \
    RUST_BACKTRACE=1 \
    BIND_ADDRESS=0.0.0.0:3000 \
    DATABASE_URL=/app/data/glimpser.db \
    STORAGE_PATH=/app/data/storage \
    FRAMES_PATH=/app/data/frames \
    VIDEOS_PATH=/app/data/videos \
    CHROME_PATH=/usr/bin/chromium \
    CHROME_DRIVER_PATH=/usr/bin/chromedriver

# Use exec form for proper signal handling
ENTRYPOINT ["/app/glimpser"]
