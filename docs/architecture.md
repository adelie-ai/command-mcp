# Architecture Documentation

System architecture and design decisions for command-mcp.

## Overview

command-mcp is a generic MCP (Model Context Protocol) server that acts as an adapter between MCP clients and arbitrary command-line tools. It provides secure execution, timeout management, and flexible configuration.

## Architecture Diagram

```
┌─────────────┐
│ MCP Client  │
└──────┬──────┘
       │
       │ JSON-RPC 2.0
       │
┌──────▼─────────────────────────────────────┐
│         Transport Layer                    │
│  ┌──────────────┐  ┌──────────────────┐   │
│  │ STDIN/STDOUT │  │   WebSocket      │   │
│  └──────────────┘  └──────────────────┘   │
└──────┬─────────────────────────────────────┘
       │
       │ Messages
       │
┌──────▼─────────────────────────────────────┐
│         MCP Server                         │
│  ┌──────────────────────────────────────┐ │
│  │  Initialize / Lifecycle Management   │ │
│  └──────────────────────────────────────┘ │
│  ┌──────────────────────────────────────┐ │
│  │  Tool Call Handler                   │ │
│  └──────────────────────────────────────┘ │
└──────┬─────────────────────────────────────┘
       │
       │ Tool Registry
       │
┌──────▼─────────────────────────────────────┐
│      Tool Registry                         │
│  ┌──────────────────────────────────────┐ │
│  │  Tool Schema Generation              │ │
│  │  MAX Constraint Validation           │ │
│  └──────────────────────────────────────┘ │
└──────┬─────────────────────────────────────┘
       │
       │ Resolved Tool Config
       │
┌──────▼─────────────────────────────────────┐
│         Executor                           │
│  ┌──────────────────────────────────────┐ │
│  │  Command Execution                    │ │
│  │  Timeout Management                   │ │
│  │  Graceful Termination                 │ │
│  │  Output Capture & Limiting            │ │
│  └──────────────────────────────────────┘ │
└──────┬─────────────────────────────────────┘
       │
       │ Process Execution
       │
┌──────▼─────────────────────────────────────┐
│      External Commands                     │
│  (mv, cp, grep, etc.)                      │
└────────────────────────────────────────────┘
```

## Components

### 1. Configuration (`config.rs`)

- **Purpose**: Parse and validate TOML configuration files
- **Key Features**:
  - Group-based organization with defaults
  - Tool-level overrides
  - MAX value constraints
  - Parameter definitions
- **Output**: `ResolvedTool` structures with all defaults applied

### 2. Error Handling (`error.rs`)

- **Purpose**: Comprehensive error types for all failure modes
- **Error Categories**:
  - Configuration errors
  - Execution errors
  - MCP protocol errors
  - Transport errors
  - Tool registry errors

### 3. Executor (`executor.rs`)

- **Purpose**: Secure command execution with resource management
- **Features**:
  - No shell execution (security)
  - Explicit argument vectors
  - Timeout handling with graceful termination
  - `stop_after` for controlled duration execution
  - STDOUT/STDERR capture
  - Output line limiting (head/tail)

### 4. Tool Registry (`tools.rs`)

- **Purpose**: Manage tools and generate MCP tool schemas
- **Features**:
  - Tool name generation (`{group}_{tool}`)
  - MCP schema generation with MAX constraints
  - Runtime override validation

### 5. MCP Server (`server.rs`)

- **Purpose**: Implement MCP protocol
- **Features**:
  - Initialize lifecycle
  - Tool call handling
  - Capability advertisement
  - Error response formatting

### 6. Transport Layer (`transport.rs`)

- **Purpose**: Handle communication with MCP clients
- **Modes**:
  - STDIN/STDOUT: JSON-RPC over stdio (auto-detects newline-delimited JSON vs `Content-Length: ...` framing)
  - WebSocket: JSON-RPC over WebSocket frames

### 7. CLI Interface (`main.rs`)

- **Purpose**: Command-line interface
- **Commands**:
  - `serve`: Run MCP server
  - `schema`: Output configuration schema

## Data Flow

### Tool Call Flow

1. Client sends tool call via transport
2. Server receives JSON-RPC message
3. Server validates tool name and parameters
4. Server validates runtime overrides against MAX values
5. Server resolves tool configuration (defaults + overrides)
6. Executor executes command with resolved configuration
7. Executor captures output and applies limits
8. Server formats response and sends to client

### Initialization Flow

1. Client sends `initialize` request
2. Server loads configuration
3. Server builds tool registry
4. Server generates tool schemas
5. Server responds with capabilities and tool list
6. Client sends `initialized` notification
7. Server enters operational phase

## Design Decisions

### Security

- **No Shell Execution**: Always use explicit argument vectors
- **Path Validation**: Commands should use absolute paths
- **Resource Limits**: MAX values prevent resource exhaustion
- **Graceful Termination**: SIGTERM/SIGINT before SIGKILL

### Configuration

- **Group-Based**: Logical organization with shared defaults
- **Override Hierarchy**: Group → Tool → Runtime (within MAX)
- **MAX Constraints**: Hard limits prevent abuse

### Error Handling

- **Custom Error Types**: Clear error categorization
- **Actionable Messages**: Errors include context for debugging
- **Error Propagation**: Errors bubble up with context

### Performance

- **Async Execution**: Tokio for concurrent operations
- **Streaming Output**: Capture output as it's produced
- **Efficient Parsing**: TOML parsing with validation

## Future Enhancements

1. **JWT Validation**: Enhanced JWT token validation with claims validation (issuer, audience, etc.)
2. **Metrics**: Execution metrics and monitoring
3. **Logging**: Structured logging for debugging
4. **Caching**: Cache tool schemas and configurations
5. **Rate Limiting**: Prevent abuse with rate limits
6. **Command Path Whitelisting**: Optional whitelist of allowed command paths for additional security

