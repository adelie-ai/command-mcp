# command-mcp - Generic MCP Script Adapter

`command-mcp` turns existing command-line programs (shell scripts, binaries, and CLIs) into an MCP server you can plug into MCP clients (like VS Code) **without rewriting them as a bespoke MCP service**.

## Security Notice (Read This)

`command-mcp` **does not vet, sandbox, or approve** the command lines you configure or the programs you execute. It provides **no built-in allow/deny or interactive approval mechanism**.

- **You are responsible** for ensuring the configured commands and binaries are safe and appropriate for your environment.
- **Treat your config as code**: review it, restrict who can edit it, and assume a malicious or careless tool definition can run destructive commands.
- **Run in a secured environment**: use least-privilege accounts, tight filesystem/network permissions, and appropriate OS/container isolation for your threat model.

## Why command-mcp?

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
- **STDIO compatibility**: Supports both newline-delimited JSON and `Content-Length` framed JSON-RPC over STDIO
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

1. Create a configuration file (see `examples/unixtools_config.toml` or start with `command-mcp config example`)

2. Choose the transport mode at runtime with `--mode` (this is **not** part of the config file).

3. Run in STDIN/STDOUT mode (for VS Code):
```bash
./target/release/command-mcp serve --config examples/unixtools_config.toml --mode stdio
```

4. Run in WebSocket mode:
```bash
./target/release/command-mcp serve --config examples/unixtools_config.toml --mode websocket --port 8080
```

### Generate Configuration Schema

```bash
# JSON Schema
./target/release/command-mcp config schema

# TOML Example
./target/release/command-mcp config example

# Markdown Documentation
./target/release/command-mcp config docs

# Curated (hand-written) Markdown Documentation
./target/release/command-mcp config docs --curated
```

## Configuration

See `examples/unixtools_config.toml` for a comprehensive example configuration with common Unix commands.
See `examples/aws_cli_config.toml` for a curated AWS CLI (aws) example configuration.

Key configuration concepts:

- **Groups**: Organize tools with shared defaults
- **Tools**: Individual commands with optional overrides
- **Parameters**: Tool-specific arguments with descriptions and examples
- **MAX Values**: Hard limits that LLMs cannot exceed
- **Runtime Overrides**: LLMs can override defaults within MAX constraints

## Logging

Traces, metrics and logs come from `mcp-core`, which installs the subscriber
and holds the guard on the standard `run`/`run_simple` entry points. This
server does not use either -- its `serve` subcommand lives alongside a
`config` subcommand, so `main` installs the subscriber itself, held for the
life of `main` so the OTLP exporters flush on exit. Full mechanics (the
metrics facade, span-close events, shutdown timing, the `OTEL_*` variable
reference) are documented once in the [mcp-core
README](https://github.com/adelie-ai/mcp-core#logging); this section covers
what is specific to `command-mcp`.

### Where it goes

**stderr, always.** The stdio transport frames JSON-RPC on stdout, so a log
line there would corrupt the protocol. This holds at every level, including
`RUST_LOG=trace`.

```bash
RUST_LOG=debug command-mcp serve --config config.toml --transport stdio
```

### The level contract, and why it matters more here

| Level | Carries |
|---|---|
| INFO | ids, counts, durations, tool names. **Never content.** |
| DEBUG | tool arguments, and the reason a tool declined or failed. |

Every configured tool wraps an operator-chosen command line, so both a
caller's arguments *and* the command an operator configured are the
highest-value content in this server. Neither reaches a span field or an
INFO line, at any log level. `RUST_LOG=debug` is what it takes to see the
assembled tool arguments (via mcp-core's own dispatch layer, sanitised and
size-capped) and a declined call's reason -- deliberate, not this server's
addition.

### What this server emits

mcp-core's dispatch layer already covers the JSON-RPC request and the tool
call: `mcp.request` and `mcp.tools.call` spans, and the `mcp.requests` /
`mcp.tools.call` / `mcp.tools.call.duration` metrics, all keyed by method or
tool name, never by argument content.

On top of that, command-mcp's own path adds:

- A `command_mcp.call_tool` span around building the CLI invocation and
  formatting the result, and a `command.execute` span around the spawn
  itself -- both carry no fields drawn from the command line or its
  arguments.
- A `command.execute` counter, labelled `tool` (the resolved
  `{group}_{tool}` name) and `outcome`: `ok`, `nonzero_exit`,
  `stopped_after`, `timeout`, `command_not_found`, or `error`. A nonzero
  exit and a `stop_after`-terminated command are both domain information
  mcp-core's own protocol-level outcome metric cannot see, since the
  JSON-RPC call itself still succeeds in both cases.
- A `command.execute.duration` histogram, labelled `tool`.

### Exporting to a collector

Off by default (`otel` feature, see `Cargo.toml`; this repo already carries
`features = ["auth"]` on the `mcp-core` dependency for websocket Bearer-token
auth, which `otel` sits alongside). With it off, no opentelemetry crate is
resolved at all. With it on, configure export with the standard
`OTEL_EXPORTER_OTLP_*` environment variables -- there are no CLI flags and no
server-specific variables. See the [mcp-core
README](https://github.com/adelie-ai/mcp-core#exporting-to-a-collector) for
the full variable reference.

```bash
cargo build --release --features otel
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \
  ./target/release/command-mcp serve --config config.toml --transport stdio
```

With no collector configured, the periodic metrics summary still writes to
stderr, so a default-feature install from `cargo install` gets real numbers
in the journal.

### Stopping

`SIGTERM` and `SIGINT` both stop the server cleanly: they end the serve loop,
flush the telemetry guard (the final metrics summary reaches stderr), and
exit 0. `SIGHUP` is not handled, so it keeps its default disposition.

Open connections are cut rather than drained, and the stop adds no wait of
its own beyond the flush -- bounded by the telemetry guard's five-second
budget. `command-mcp`'s `main` wires this itself, because its `serve`
subcommand lives alongside `config` and so cannot use `mcp-core`'s
`run`/`run_simple` (the entry point that installs this for the eleven other
MCP servers in the fleet). The full mechanics -- why `SIGHUP` is refused, why
open connections are cut, how long a stop takes, and why the process ends
itself rather than returning -- are documented once in the [mcp-core
README](https://github.com/adelie-ai/mcp-core#stopping-a-server).

## Docker

```bash
# Build
docker build -t command-mcp .

# Run in stdio mode
docker run -i command-mcp serve --mode stdio

# Run in websocket mode
docker run -p 8080:8080 command-mcp serve --mode websocket --port 8080
```

The container defaults `COMMAND_MCP_CONFIG` to `/example_configs/echo_config.toml`. To mount your own config, mount into `/configs` and set `COMMAND_MCP_CONFIG`:

```bash
docker run -i \
  -v /path/to/config.toml:/configs/config.toml:ro \
  -e COMMAND_MCP_CONFIG=/configs/config.toml \
  command-mcp serve --mode stdio
```

## VS Code Integration

See `examples/vscode_mcp_config.json` and `examples/vscode_mcp_config_examples.md` for detailed VS Code MCP configuration examples.

Quick example:
```json
{
  "mcpServers": {
    "command-mcp": {
      "command": "command-mcp",
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

