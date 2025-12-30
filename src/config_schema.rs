#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Configuration schema generation for LLM assistance

use crate::error::Result;
use schemars::schema::{InstanceType, RootSchema, Schema, SchemaObject, SingleOrVec};
use schemars::schema_for;
use std::collections::HashMap;
use std::io::Write;

// #region agent log
fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": ts
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/home/dave/projects/genmcp/.cursor/debug.log")
    {
        let _ = writeln!(f, "{}", payload);
    }
}
// #endregion

/// Output generated JSON Schema for the TOML configuration structure.
pub fn output_generated_schema() -> Result<()> {
    let schema = schema_for!(crate::config::ConfigToml);
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}

/// Output an example TOML configuration file.
pub fn output_example_config() -> Result<()> {
    // Keep this as an alias for backwards compatibility: example output is always
    // generated from Rust structs so it stays in sync and doesn't require files
    // to exist at build time (important for container builds).
    output_generated_example_config()
}

/// Output a minimal TOML configuration file generated from the Rust config structures.
///
/// This output is intentionally "mechanical" (no comments) but stays in sync with the
/// Rust structures: if you add a new required field, this code must be updated to compile.
pub fn output_generated_example_config() -> Result<()> {
    agent_debug_log(
        "H_generated_example",
        "src/config_schema.rs:output_generated_example_config",
        "enter",
        serde_json::json!({}),
    );
    let config = build_generated_example();
    agent_debug_log(
        "H_generated_example",
        "src/config_schema.rs:output_generated_example_config",
        "built example",
        serde_json::json!({
            "groups": config.groups.len(),
            "has_websocket_auth": config.websocket_auth.is_some()
        }),
    );
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

/// Output Markdown documentation generated from the Rust config structures (stays in sync).
pub fn output_docs_generated() -> Result<()> {
    let root = schema_for!(crate::config::ConfigToml);
    let docs = render_markdown_docs_from_schema(&root);
    println!("{docs}");
    Ok(())
}

/// Output Markdown documentation for the configuration file format (hand-written).
pub fn output_docs_curated() -> Result<()> {
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

fn render_markdown_docs_from_schema(root: &RootSchema) -> String {
    let mut out = String::new();

    out.push_str("# genmcp Configuration (generated)\n\n");
    out.push_str(
        "This documentation is generated from the Rust configuration structs (field doc comments + schema), so it stays in sync with the running binary.\n\n",
    );
    out.push_str("## Quick commands\n\n");
    out.push_str("- `genmcp config schema` (generated JSON Schema)\n");
    out.push_str("- `genmcp config example` (curated example TOML)\n");
    out.push_str(
        "- `genmcp config example` (struct-synced TOML example; no comments)\n",
    );
    out.push_str("- `genmcp config docs --curated` (hand-written docs)\n\n");

    out.push_str("## Top-level keys\n\n");
    if let Some(obj) = root.schema.object.as_ref() {
        let required = &obj.required;
        for (name, schema) in &obj.properties {
            render_field(&mut out, root, name, schema, required.contains(name));
        }
    } else {
        out.push_str("_Unexpected: root schema is not an object._\n");
    }

    out.push_str("\n## Definitions\n\n");
    for def in ["Group", "Tool", "Parameter", "WebSocketAuth"] {
        if let Some(schema) = root.definitions.get(def) {
            out.push_str(&format!("### `{def}`\n\n"));
            render_object_fields(&mut out, root, schema);
            out.push('\n');
        }
    }

    out
}

fn render_field(out: &mut String, root: &RootSchema, name: &str, schema: &Schema, required: bool) {
    let (ty, desc, enum_vals) = describe_schema(root, schema);
    let req = if required { "Required" } else { "Optional" };
    out.push_str(&format!("- **`{name}`** ({req}, `{ty}`)"));
    if let Some(d) = desc {
        out.push_str(&format!(": {d}"));
    }
    out.push('\n');
    if !enum_vals.is_empty() {
        out.push_str("  - **Allowed values**: ");
        for (i, v) in enum_vals.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("`{v}`"));
        }
        out.push('\n');
    }
}

fn render_object_fields(out: &mut String, root: &RootSchema, schema: &Schema) {
    let schema = deref_schema(root, schema);
    let Schema::Object(obj) = schema else {
        out.push_str("_Not an object schema._\n");
        return;
    };
    let Some(o) = obj.object.as_ref() else {
        out.push_str("_Not an object schema._\n");
        return;
    };

    if o.properties.is_empty() {
        out.push_str("_No fields._\n");
        return;
    }

    for (name, field_schema) in &o.properties {
        render_field(out, root, name, field_schema, o.required.contains(name));
    }
}

fn describe_schema(root: &RootSchema, schema: &Schema) -> (String, Option<String>, Vec<String>) {
    let schema = deref_schema(root, schema);
    match schema {
        Schema::Object(obj) => describe_schema_object(root, obj),
        Schema::Bool(true) => ("any".to_string(), None, Vec::new()),
        Schema::Bool(false) => ("never".to_string(), None, Vec::new()),
    }
}

fn describe_schema_object(
    root: &RootSchema,
    obj: &SchemaObject,
) -> (String, Option<String>, Vec<String>) {
    let desc = obj
        .metadata
        .as_ref()
        .and_then(|m| m.description.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Try to extract enum values if present.
    let enum_vals = obj
        .enum_values
        .as_ref()
        .map(|vals| {
            vals.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Prefer instance_type when available; otherwise fall back to validation hints.
    let ty = if let Some(it) = obj.instance_type.as_ref() {
        instance_type_to_string(it)
    } else if obj.object.is_some() {
        "object".to_string()
    } else if obj.array.is_some() {
        "array".to_string()
    } else if obj.string.is_some() {
        "string".to_string()
    } else if obj.number.is_some() {
        "number".to_string()
    } else if obj
        .subschemas
        .as_ref()
        .and_then(|s| s.any_of.as_ref())
        .is_some()
    {
        // Common for Option<T>: anyOf [T, null]
        let any_of = obj
            .subschemas
            .as_ref()
            .and_then(|s| s.any_of.as_ref())
            .unwrap();
        let mut parts = Vec::new();
        for s in any_of {
            let (t, _, _) = describe_schema(root, s);
            if !parts.contains(&t) {
                parts.push(t);
            }
        }
        parts.join(" | ")
    } else {
        "any".to_string()
    };

    (ty, desc, enum_vals)
}

fn instance_type_to_string(it: &SingleOrVec<InstanceType>) -> String {
    match it {
        SingleOrVec::Single(t) => instance_type_one_to_string(**t),
        SingleOrVec::Vec(v) => v
            .iter()
            .map(|t| instance_type_one_to_string(*t))
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn instance_type_one_to_string(t: InstanceType) -> String {
    match t {
        InstanceType::Null => "null",
        InstanceType::Boolean => "bool",
        InstanceType::Object => "object",
        InstanceType::Array => "array",
        InstanceType::Number => "number",
        InstanceType::String => "string",
        InstanceType::Integer => "integer",
    }
    .to_string()
}

fn deref_schema<'a>(root: &'a RootSchema, schema: &'a Schema) -> &'a Schema {
    let Schema::Object(obj) = schema else {
        return schema;
    };

    let Some(reference) = obj.reference.as_ref() else {
        return schema;
    };

    // schemars uses JSON pointer style refs like "#/definitions/TypeName"
    let Some(name) = reference.strip_prefix("#/definitions/") else {
        return schema;
    };

    root.definitions.get(name).unwrap_or(schema)
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
    fn test_output_markdown_docs_generated() {
        assert!(output_docs_generated().is_ok());
    }

    #[test]
    fn test_output_markdown_docs_curated() {
        assert!(output_docs_curated().is_ok());
    }

    #[test]
    fn test_output_schema_valid_formats() {
        assert!(output_generated_schema().is_ok());
        assert!(output_example_config().is_ok());
        assert!(output_docs_generated().is_ok());
        assert!(output_docs_curated().is_ok());
    }
}
