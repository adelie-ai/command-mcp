# Multi-stage build for genmcp

# Build stage
FROM rust:1.92 as builder

WORKDIR /build

# Copy manifest files
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 genmcp

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/genmcp /app/genmcp

# Copy example config
COPY examples/config.toml /app/examples/config.toml

# Set ownership
RUN chown -R genmcp:genmcp /app

USER genmcp

# Expose WebSocket port
EXPOSE 8080

# Default to stdio mode
ENTRYPOINT ["/app/genmcp"]
CMD ["serve", "--config", "/app/examples/config.toml", "--mode", "stdio"]

