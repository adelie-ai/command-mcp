#![deny(warnings)]
#![allow(dead_code)] // Error types will be used as modules are implemented

// Error types for the gen-mcp crate

use thiserror::Error;

/// Main error type for the gen-mcp application
#[derive(Error, Debug)]
pub enum GenMcpError {
    /// Configuration parsing or validation errors
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// JSON serialization/deserialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML serialization errors
    #[error("TOML error: {0}")]
    Toml(#[from] toml::ser::Error),

    /// Command execution errors
    #[error("Execution error: {0}")]
    Execution(#[from] ExecutionError),

    /// MCP protocol errors
    #[error("MCP protocol error: {0}")]
    Mcp(#[from] McpError),

    /// Errors surfaced by the mcp-core server runtime (transport, serving).
    #[error("server error: {0}")]
    Server(#[from] mcp_core::Error),

    /// Tool registry errors
    #[error("Tool registry error: {0}")]
    ToolRegistry(#[from] ToolRegistryError),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
    /// TOML parsing error
    #[error("Failed to parse TOML: {0}")]
    ParseToml(#[from] toml::de::Error),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid field value
    #[error("Invalid value for field '{field}': {message}")]
    InvalidValue { field: String, message: String },

    /// File not found
    #[error("Configuration file not found: {0}")]
    FileNotFound(String),

    /// Invalid timeout value
    #[error("Invalid timeout value: {0} seconds (must be positive)")]
    InvalidTimeout(u64),

    /// Invalid MAX value (MAX must be >= default)
    #[error("Invalid MAX value for '{field}': MAX ({max}) must be >= default ({default})")]
    InvalidMax {
        field: String,
        default: u64,
        max: u64,
    },

    /// Invalid signal name
    #[error("Invalid termination signal: {0} (must be SIGTERM or SIGINT)")]
    InvalidSignal(String),

    /// Duplicate tool name
    #[error("Duplicate tool name: {0}")]
    DuplicateToolName(String),
}

/// Command execution errors
#[derive(Error, Debug)]
pub enum ExecutionError {
    /// Command execution failed
    #[error("Command execution failed: {command}")]
    CommandFailed {
        command: String,
        exit_code: Option<i32>,
        stderr: String,
    },

    /// Command timeout
    #[error("Command timed out after {timeout} seconds: {command}")]
    Timeout { command: String, timeout: u64 },

    /// Process was stopped after stop_after duration (this is success, not error)
    /// Note: This should not be used as an error, but included for completeness
    #[error("Process stopped after {duration} seconds: {command}")]
    StoppedAfter { command: String, duration: u64 },

    /// Failed to send termination signal
    #[error("Failed to send termination signal to process: {0}")]
    SignalFailed(String),

    /// Command not found
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    /// Permission denied
    #[error("Permission denied executing command: {0}")]
    PermissionDenied(String),

    /// Invalid arguments
    #[error("Invalid arguments for command: {0}")]
    InvalidArguments(String),
}

/// MCP-domain errors raised while preparing a tool call. Protocol/transport
/// concerns (version negotiation, JSON-RPC dispatch, framing, websocket auth)
/// now live in mcp-core; only domain-level faults remain here.
#[derive(Error, Debug)]
pub enum McpError {
    /// Runtime override exceeds MAX value
    #[error("Runtime override '{field}' ({value}) exceeds MAX value ({max})")]
    OverrideExceedsMax { field: String, value: u64, max: u64 },
}

/// Tool registry errors
#[derive(Error, Debug)]
pub enum ToolRegistryError {
    /// Tool already registered
    #[error("Tool already registered: {0}")]
    DuplicateTool(String),

    /// Tool not found in registry
    #[error("Tool not found in registry: {0}")]
    ToolNotFound(String),

    /// Invalid tool configuration
    #[error("Invalid tool configuration: {0}")]
    InvalidConfig(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, GenMcpError>;
