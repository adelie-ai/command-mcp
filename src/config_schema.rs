#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Configuration schema generation for LLM assistance

use crate::error::Result;
use schemars::schema_for;
use std::collections::HashMap;

/// Output generated JSON Schema for the TOML configuration structure.
pub fn output_generated_schema() -> Result<()> {
    let schema = schema_for!(crate::config::ConfigToml);
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

/// Output an example TOML configuration file.
pub fn output_example_config() -> Result<()> {
    println!("{}", include_str!("../examples/config.toml"));
    Ok(())
}

/// Output a minimal TOML configuration file generated from the Rust config structures.
///
/// This output is intentionally "mechanical" (no comments) but stays in sync with the
/// Rust structures: if you add a new required field, this code must be updated to compile.
pub fn output_generated_example_config() -> Result<()> {
    let config = build_generated_example();
    println!("{}", toml::to_string_pretty(&config)?);
    Ok(())
}

fn build_generated_example() -> crate::config::ConfigToml {
    use crate::config::{ConfigToml, Group, Parameter, Tool};

    let mut parameters = HashMap::new();
    parameters.insert(
        "text".to_string(),
        Parameter {
            description: "Text to print".to_string(),
            example: Some("hello from genmcp".to_string()),
            flag: None,
            takes_value: false,
            required: true,
        },
    );

    let tool = Tool {
        name: "echo".to_string(),
        description: "Example tool: echo text (replace with your real command)".to_string(),
        command: "/bin/echo".to_string(),
        arg_order: Some(vec!["text".to_string()]),
        timeout: Some(30),
        timeout_max: Some(300),
        stop_after: None,
        stop_after_max: None,
        termination_signal: Some("SIGTERM".to_string()),
        termination_grace_period: Some(3),
        output_head_lines: Some(200),
        output_tail_lines: Some(200),
        output_head_lines_max: Some(2000),
        output_tail_lines_max: Some(2000),
        stderr_lines: Some(200),
        stderr_lines_max: Some(2000),
        parameters,
    };

    let group = Group {
        default_timeout: Some(30),
        default_timeout_max: Some(300),
        default_stop_after: None,
        default_stop_after_max: None,
        default_termination_signal: Some("SIGTERM".to_string()),
        default_termination_grace_period: Some(3),
        default_output_head_lines: Some(200),
        default_output_tail_lines: Some(200),
        default_output_head_lines_max: Some(2000),
        default_output_tail_lines_max: Some(2000),
        default_stderr_lines: Some(200),
        default_stderr_lines_max: Some(2000),
        tools: vec![tool],
    };

    let mut groups = HashMap::new();
    groups.insert("example".to_string(), group);

    ConfigToml {
        groups,
        websocket_auth: None,
    }
}

/// Output Markdown documentation for the configuration file format.
pub fn output_docs() -> Result<()> {
    let docs = r#"# genmcp Configuration Schema

## Overview

The genmcp configuration file uses TOML format and organizes tools into groups.

## Group Configuration

Each group can have default values that apply to all tools in that group:

- `default_timeout`: Default timeout in seconds
- `default_timeout_max`: Maximum timeout (LLM cannot exceed)
- `default_stop_after`: Default stop_after duration (0 = disabled)
- `default_stop_after_max`: Maximum stop_after duration
- `default_termination_signal`: Default signal (SIGTERM or SIGINT)
- `default_termination_grace_period`: Grace period in seconds
- `default_output_head_lines`: Default head lines limit
- `default_output_tail_lines`: Default tail lines limit
- `default_output_head_lines_max`: Maximum head lines
- `default_output_tail_lines_max`: Maximum tail lines
- `default_stderr_lines`: Default stderr lines to capture
- `default_stderr_lines_max`: Maximum stderr lines

## Tool Configuration

Each tool can override group defaults:

- `name`: Base tool name (final name: `{group_name}_{tool_name}`)
- `description`: Description for LLM
- `command`: Command to execute
- `arg_order` (optional): Explicit parameter evaluation order when building CLI args
- `timeout`, `timeout_max`: Override group timeout settings
- `stop_after`, `stop_after_max`: Override group stop_after settings
- `termination_signal`: Override group termination signal
- `termination_grace_period`: Override group grace period
- `output_head_lines`, `output_head_lines_max`: Override output limits
- `output_tail_lines`, `output_tail_lines_max`: Override output limits
- `stderr_lines`, `stderr_lines_max`: Override stderr limits
- `parameters`: Tool-specific parameters

## Parameters

Each parameter has:
- `description`: Parameter description
- `example`: Example value (optional)
- `flag` (optional): Emit this CLI flag when the parameter is provided (e.g. `-r`, `-n`)
- `takes_value` (optional, boolean): If `true`, emit `flag` followed by the parameter value (e.g. `-n 50`)
- `required`: Whether parameter is required (default: false)

## WebSocket Authentication Configuration

Optional `[websocket_auth]` section for WebSocket mode:

- `enabled` (optional, boolean): Enable JWT authentication. Default: `true` if section exists
- `secret` (optional, string): JWT secret key for token validation. Required if `enabled = true`

**To disable authentication entirely**, omit the `[websocket_auth]` section from your configuration file.

**CLI Override**: The `--jwt-secret` CLI option takes precedence over the config file setting.
"#;
    println!("{}", docs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_json_schema() {
        assert!(output_generated_schema().is_ok());
    }

    #[test]
    fn test_generated_schema_includes_expected_fields() {
        let schema = schema_for!(crate::config::ConfigToml);
        let s = serde_json::to_string(&schema).expect("schema should serialize to JSON");

        assert!(s.contains("\"groups\""));
        assert!(s.contains("\"websocket_auth\""));
        assert!(s.contains("\"jwks_url\""));
        assert!(s.contains("SIGTERM"));
        assert!(s.contains("SIGINT"));
    }

    #[test]
    fn test_output_toml_example() {
        assert!(output_example_config().is_ok());
    }

    #[test]
    fn test_output_generated_example_config() {
        assert!(output_generated_example_config().is_ok());
    }

    #[test]
    fn test_output_markdown_docs() {
        assert!(output_docs().is_ok());
    }

    #[test]
    fn test_output_schema_valid_formats() {
        assert!(output_generated_schema().is_ok());
        assert!(output_example_config().is_ok());
        assert!(output_docs().is_ok());
    }
}
