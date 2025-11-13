# Stage 1: Build Rust binaries
FROM rust:1.91-slim as rust-builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY coordinator ./coordinator
COPY participant ./participant
COPY common ./common
COPY api-gateway ./api-gateway
COPY electrum-servers.toml ./

# Build release binaries
RUN cargo build --release --bin api-gateway --bin coordinator

# Stage 2: Build Web UI
FROM node:20-alpine as ui-builder

WORKDIR /app

# Copy package files
COPY web-ui/package.json web-ui/package-lock.json ./
RUN npm ci --no-audit

# Copy source and build
COPY web-ui ./
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

# Copy Rust binaries
COPY --from=rust-builder /build/target/release/api-gateway /app/
COPY --from=rust-builder /build/target/release/coordinator /app/
COPY --from=rust-builder /build/electrum-servers.toml /app/

# Copy web UI
COPY --from=ui-builder /app/dist /app/web

# Create data directories
RUN mkdir -p /data /config

# Environment defaults
ENV API_HOST=0.0.0.0
ENV API_PORT=3000
ENV DATABASE_URL=sqlite:///data/coordinator.db
ENV WEB_DIR=/app/web
ENV NETWORK=bitcoin
ENV MIN_PARTICIPANTS=2
ENV MAX_PARTICIPANTS=10
ENV TIMEOUT_SECONDS=300

# Expose ports
EXPOSE 3000

# Copy entrypoint script
COPY docker/entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh

ENTRYPOINT ["/app/entrypoint.sh"]
