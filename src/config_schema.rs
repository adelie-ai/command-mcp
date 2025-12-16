#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Configuration schema generation for LLM assistance

use crate::error::Result;

/// Output configuration schema in the specified format
pub fn output_schema(format: &str) -> Result<()> {
    match format {
        "json" => output_json_schema(),
        "toml" => output_toml_example(),
        "markdown" => output_markdown_docs(),
        _ => {
            eprintln!("Invalid format: {}. Must be 'json', 'toml', or 'markdown'", format);
            #[cfg(not(test))]
            {
                std::process::exit(1);
            }
            #[cfg(test)]
            {
                panic!("Invalid format: {}", format);
            }
        }
    }
}

fn output_json_schema() -> Result<()> {
    // Simplified JSON Schema structure to avoid recursion limit
    let schema = r#"{
  "type": "object",
  "properties": {
    "groups": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "default_timeout": { "type": "integer" },
          "default_timeout_max": { "type": "integer" },
          "default_stop_after": { "type": "integer" },
          "default_stop_after_max": { "type": "integer" },
          "default_termination_signal": { "type": "string", "enum": ["SIGTERM", "SIGINT"] },
          "default_termination_grace_period": { "type": "integer" },
          "default_output_head_lines": { "type": "integer" },
          "default_output_tail_lines": { "type": "integer" },
          "default_output_head_lines_max": { "type": "integer" },
          "default_output_tail_lines_max": { "type": "integer" },
          "default_stderr_lines": { "type": "integer" },
          "default_stderr_lines_max": { "type": "integer" },
          "tools": {
            "type": "array",
            "items": {
              "type": "object",
              "required": ["name", "description", "command"],
              "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "command": { "type": "string" },
                "timeout": { "type": "integer" },
                "timeout_max": { "type": "integer" },
                "stop_after": { "type": "integer" },
                "stop_after_max": { "type": "integer" },
                "termination_signal": { "type": "string", "enum": ["SIGTERM", "SIGINT"] },
                "termination_grace_period": { "type": "integer" },
                "output_head_lines": { "type": "integer" },
                "output_tail_lines": { "type": "integer" },
                "output_head_lines_max": { "type": "integer" },
                "output_tail_lines_max": { "type": "integer" },
                "stderr_lines": { "type": "integer" },
                "stderr_lines_max": { "type": "integer" },
                "parameters": {
                  "type": "object",
                  "additionalProperties": {
                    "type": "object",
                    "properties": {
                      "description": { "type": "string" },
                      "example": { "type": "string" },
                      "required": { "type": "boolean" }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}"#;
    println!("{}", schema);
    Ok(())
}

fn output_toml_example() -> Result<()> {
    let example = r#"# Example genmcp configuration file

[groups.file_ops]
default_timeout = 30
default_timeout_max = 300
default_output_head_lines = 100
default_output_tail_lines = 100

  [[groups.file_ops.tools]]
  name = "mv"
  description = "Move or rename files and directories"
  command = "/bin/mv"
  
    [groups.file_ops.tools.parameters.source]
    description = "Source file or directory"
    required = true
    
    [groups.file_ops.tools.parameters.dest]
    description = "Destination file or directory"
    required = true
"#;
    println!("{}", example);
    Ok(())
}

fn output_markdown_docs() -> Result<()> {
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
- `required`: Whether parameter is required (default: false)
"#;
    println!("{}", docs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_json_schema() {
        assert!(output_json_schema().is_ok());
    }

    #[test]
    fn test_output_toml_example() {
        assert!(output_toml_example().is_ok());
    }

    #[test]
    fn test_output_markdown_docs() {
        assert!(output_markdown_docs().is_ok());
    }

    #[test]
    #[should_panic(expected = "Invalid format")]
    fn test_output_schema_invalid_format() {
        // This will exit, but we test that it at least doesn't panic on other errors
        // The actual exit behavior is tested in integration tests
        let _ = output_schema("invalid");
    }

    #[test]
    fn test_output_schema_valid_formats() {
        assert!(output_schema("json").is_ok());
        assert!(output_schema("toml").is_ok());
        assert!(output_schema("markdown").is_ok());
    }
}
