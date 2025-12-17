# Example Configurations

This directory contains example configuration files demonstrating various use cases for genmcp.

## Configuration Files

### `config.toml`
Basic example with common Unix/shell commands (mv, cp, awk, sed, wc, grep, etc.). Includes WebSocket auth configuration comments.

### `docker_config.toml`
Example configuration for Docker commands (`docker run`, `docker ps`, `docker stop`, etc.). Demonstrates how to expose Docker CLI as MCP tools.

### `podman_config.toml`
Example configuration for Podman commands (`podman run`, `podman ps`, `podman stop`, etc.). This is a Podman-adapted version of the Docker example.

### `kubernetes_config.toml`
Example configuration for Kubernetes commands via `kubectl` (`kubectl get`, `kubectl apply`, `kubectl logs`, etc.).

### `just_config.toml`
Configuration for running `just` (justfile runner) targets. Includes tools for:
- Running just targets
- Listing available targets
- Showing target recipes
- Running targets in specific directories
- Running with environment variables
- Dry-run mode

### `just_with_auth.toml`
Complete example combining `just` command execution with WebSocket JWT authentication. This is a production-ready template showing:
- Secure JWT authentication configuration
- Just command tools with proper timeouts
- Comments explaining development vs production usage

### `websocket_auth_config.toml`
Focused example demonstrating all WebSocket authentication options:
- Enabling JWT authentication with secret
- Disabling authentication
- Omitting authentication entirely
- CLI override behavior

## WebSocket Authentication

All example configs that include `[websocket_auth]` demonstrate how to configure JWT authentication:

```toml
[websocket_auth]
enabled = true
secret = "your-secret-key-here"
```

**To disable authentication:**
1. Omit the `[websocket_auth]` section entirely, OR
2. Set `enabled = false`

**CLI Override:**
The `--jwt-secret` CLI option takes precedence over the config file:
```bash
genmcp serve --config config.toml --mode websocket --jwt-secret "cli-secret"
```

## Usage Examples

### Running with STDIN/STDOUT (VS Code)
```bash
genmcp serve --config examples/just_with_auth.toml --mode stdio
```

### Running with WebSocket (no auth for development)
```bash
genmcp serve --config examples/just_config.toml --mode websocket --port 8080
```

### Running with WebSocket (with JWT auth)
```bash
genmcp serve --config examples/just_with_auth.toml --mode websocket --port 8080
```

### Overriding JWT secret via CLI
```bash
genmcp serve --config examples/just_with_auth.toml --mode websocket --jwt-secret "$JWT_SECRET"
```

## Generating Secure Secrets

For production, generate a secure JWT secret:

```bash
# Using OpenSSL
openssl rand -base64 32

# Using /dev/urandom
head -c 32 /dev/urandom | base64
```

## See Also

- [Configuration Reference](../docs/configuration.md) - Complete configuration documentation
- [Deployment Guide](../docs/deployment.md) - Deployment instructions
- [Architecture](../docs/architecture.md) - System architecture


