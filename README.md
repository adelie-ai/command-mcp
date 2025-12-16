# genmcp - Generic MCP Script Adapter

A generic Model Context Protocol (MCP) server that acts as an adapter to arbitrary command-line tools and scripts. It supports dual-mode operation (STDIN/STDOUT for VS Code integration, WebSocket for web services) and provides a flexible TOML-based configuration system.

## Features

- **Dual Transport Modes**: STDIN/STDOUT (for VS Code) and WebSocket (for web services)
- **TOML Configuration**: Flexible, group-based configuration with defaults and overrides
- **Secure Execution**: No shell execution, explicit argument vectors, proper escaping
- **Timeout Management**: Configurable timeouts with graceful termination (SIGTERM/SIGINT → SIGKILL)
- **Stop After Feature**: Controlled duration execution for long-running processes (e.g., `tail -f`)
- **Output Management**: Head/tail line limits, STDERR capture with configurable limits
- **MAX Constraints**: Prevent LLM from exceeding resource limits
- **Schema Generation**: Output configuration schema in JSON, TOML, or Markdown format

## Quick Start

### Installation

```bash
cargo build --release
```

### Basic Usage

1. Create a configuration file (see `examples/config.toml`)

2. Run in STDIN/STDOUT mode (for VS Code):
```bash
./target/release/genmcp serve --config examples/config.toml --mode stdio
```

3. Run in WebSocket mode:
```bash
./target/release/genmcp serve --config examples/config.toml --mode websocket --port 8080
```

### Generate Configuration Schema

```bash
# JSON Schema
./target/release/genmcp schema --format json

# TOML Example
./target/release/genmcp schema --format toml

# Markdown Documentation
./target/release/genmcp schema --format markdown
```

## Configuration

See `examples/config.toml` for a comprehensive example configuration with common Unix commands.

Key configuration concepts:

- **Groups**: Organize tools with shared defaults
- **Tools**: Individual commands with optional overrides
- **Parameters**: Tool-specific arguments with descriptions and examples
- **MAX Values**: Hard limits that LLMs cannot exceed
- **Runtime Overrides**: LLMs can override defaults within MAX constraints

## Docker

```bash
# Build
docker build -t genmcp .

# Run in stdio mode
docker run -i genmcp serve --config /app/examples/config.toml --mode stdio

# Run in websocket mode
docker run -p 8080:8080 genmcp serve --config /app/examples/config.toml --mode websocket --port 8080
```

## Documentation

- [Configuration Reference](docs/configuration.md) - Complete configuration guide
- [Deployment Guide](docs/deployment.md) - Docker and bare metal deployment
- [Architecture](docs/architecture.md) - System design and components
- [Development Guide](docs/development.md) - Development setup and contribution

## License

[Add license information]

## Contributing

[Add contribution guidelines]

