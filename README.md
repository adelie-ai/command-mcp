# genmcp - Generic MCP Script Adapter

`genmcp` turns existing command-line programs (shell scripts, binaries, and CLIs) into an MCP server you can plug into MCP clients (like VS Code) **without rewriting them as a bespoke MCP service**.

## Why genmcp?

- **Turn arbitrary scripts into MCP tools**: Wrap your existing shell scripts and internal tooling behind MCP, with structured tool definitions and parameters.
- **Spin up MCP servers for existing CLIs quickly**: Point at a CLI you already trust, describe its arguments once in TOML, and expose it as an MCP tool set.
- **Deploy the same config two ways**: Run locally via **STDIN/STDOUT** (VS Code integration) or host via **WebSocket** (service deployment).
- **Safer execution by default**: No shell execution; commands run with explicit argument vectors.
- **Operational guardrails**: Timeouts with graceful termination, `stop_after` for long-running commands, and output head/tail limits.
- **LLM-safe limits**: Hard MAX constraints plus bounded runtime overrides to keep tools within resource budgets.

Common scenarios:

- **You already have a CLI** (or a pile of scripts) and want MCP support without a rewrite
- **You want parameterized tools** with descriptions/examples that clients can surface nicely
- **You want local + hosted** operation from the same configuration

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
./target/release/genmcp config schema

# TOML Example
./target/release/genmcp config example

# Generated TOML example (machine-friendly, stays in sync with Rust structs; no comments)
./target/release/genmcp config example --generated

# Markdown Documentation
./target/release/genmcp config docs

# Curated (hand-written) Markdown Documentation
./target/release/genmcp config docs --curated
```

## Configuration

See `examples/config.toml` for a comprehensive example configuration with common Unix commands.
See `examples/aws_cli_config.toml` for a curated AWS CLI (aws) example configuration.

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

## VS Code Integration

See `examples/vscode_mcp_config.json` and `examples/vscode_mcp_config_examples.md` for detailed VS Code MCP configuration examples.

Quick example:
```json
{
  "mcpServers": {
    "genmcp": {
      "command": "genmcp",
      "args": [
        "serve",
        "--config",
        "/path/to/config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

## Documentation

- [Configuration Reference](docs/configuration.md) - Complete configuration guide
- [Deployment Guide](docs/deployment.md) - Docker and bare metal deployment
- [Architecture](docs/architecture.md) - System design and components
- [Development Guide](docs/development.md) - Development setup and contribution
- [VS Code Configuration](examples/vscode_mcp_config_examples.md) - VS Code MCP setup guide

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

When distributing, you must also retain the attribution notices in [NOTICE](NOTICE) (if applicable for your distribution).

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed under the Apache License, Version 2.0, without any additional terms or conditions.

