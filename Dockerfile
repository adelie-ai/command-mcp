# Multi-stage build for command-mcp

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
RUN useradd -m -u 1000 command-mcp

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/command-mcp /app/command-mcp

# Mount point for external configuration (recommended)
RUN mkdir -p /configs /example_configs

# Copy example configs into the image for reference and quick-start
COPY examples/ /example_configs/

# Set ownership
RUN chown -R command-mcp:command-mcp /app /configs /example_configs

USER command-mcp

# Expose WebSocket port
EXPOSE 8080

# Default to stdio mode
ENTRYPOINT ["/app/command-mcp"]
# Default config can be overridden by setting COMMAND_MCP_CONFIG or passing --config.
ENV COMMAND_MCP_CONFIG=/example_configs/echo_config.toml
CMD ["serve", "--mode", "stdio"]

