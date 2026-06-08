# Deployment Guide

Guide for deploying gen-mcp in various environments.

## Docker Deployment

### Building the Image

```bash
docker build -t gen-mcp .
```

### Running in STDIN/STDOUT Mode

For VS Code integration or other stdio-based MCP clients:

```bash
docker run -i gen-mcp serve --mode stdio
```

### Running in WebSocket Mode

```bash
docker run -p 8080:8080 gen-mcp serve \
  --mode websocket \
  --port 8080 \
  --host 0.0.0.0
```

### Custom Configuration

Mount your configuration file:

```bash
docker run -i -v /path/to/config.toml:/app/config.toml \
  -e GENMCP_CONFIG=/app/config.toml \
  gen-mcp serve --mode stdio
```

### Environment Variables

The Docker image runs as a non-root user (`gen-mcp`, UID 1000) for security.

## Bare Metal Installation

### Prerequisites

- Rust 1.92 or later
- Cargo

### Building from Source

```bash
git clone <repository-url>
cd gen-mcp
cargo build --release
```

The binary will be at `target/release/gen-mcp`.

### Installation

```bash
# Install to /usr/local/bin
sudo cp target/release/gen-mcp /usr/local/bin/

# Or install to user directory
cp target/release/gen-mcp ~/.local/bin/
```

### Running

`gen-mcp` uses the same TOML configuration for both transports. The transport is selected at runtime with `--mode`:

- `--mode stdio` (default): STDIN/STDOUT transport (typical for VS Code integration)
- `--mode websocket`: WebSocket transport (typical for hosted deployments)

```bash
# STDIN/STDOUT mode
gen-mcp serve --config /path/to/config.toml --mode stdio

# WebSocket mode
gen-mcp serve --config /path/to/config.toml --mode websocket --port 8080
```

## Systemd Service

Create `/etc/systemd/system/gen-mcp.service`:

```ini
[Unit]
Description=Generic MCP Script Adapter
After=network.target

[Service]
Type=simple
User=gen-mcp
WorkingDirectory=/opt/gen-mcp
ExecStart=/usr/local/bin/gen-mcp serve \
  --config /opt/gen-mcp/config.toml \
  --mode websocket \
  --port 8080
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl enable gen-mcp
sudo systemctl start gen-mcp
```

## VS Code Integration

Configure in VS Code settings:

```json
{
  "mcp.servers": {
    "gen-mcp": {
      "command": "gen-mcp",
      "args": ["serve", "--config", "/path/to/config.toml", "--mode", "stdio"]
    }
  }
}
```

## WebSocket Client Connection

Connect to WebSocket server:

```javascript
const ws = new WebSocket('ws://localhost:8080');
ws.onopen = () => {
  // Send MCP initialize request
  ws.send(JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "client", version: "1.0.0" }
    }
  }));
};
```

## Security Considerations

1. **File Permissions**: Ensure configuration file has appropriate permissions
2. **Command Paths**: Use absolute paths for commands in configuration
3. **Network Access**: In WebSocket mode, consider firewall rules
4. **Transport Security (TLS)**: WebSocket mode is plain `ws://` by default. For real deployments, run behind TLS termination (reverse proxy/ingress) and use `wss://` externally so Bearer tokens are not sent in cleartext.
5. **Authentication**: WebSocket auth supports JWT signature verification (shared secret or JWKS via OIDC/JWKS URL), but it does not currently expose detailed policy controls (e.g., audience/issuer allowlists, required claims) and should be reviewed for your threat model.
6. **User Permissions**: Run with minimal required privileges

## Troubleshooting

### Permission Denied

Ensure the binary and configuration file have appropriate permissions:

```bash
chmod +x gen-mcp
chmod 644 config.toml
```

### Port Already in Use

Change the port:

```bash
gen-mcp serve --config config.toml --mode websocket --port 8081
```

### Configuration Errors

Validate configuration:

```bash
gen-mcp config example > /tmp/config.toml
# Compare with your config
```

