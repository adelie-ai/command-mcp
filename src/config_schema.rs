#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Configuration schema generation for LLM assistance

use crate::error::Result;
use schemars::schema::{InstanceType, RootSchema, Schema, SchemaObject, SingleOrVec};
use schemars::schema_for;

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

/// Output a minimal, modern TOML configuration file.
///
/// The example is deliberately short and copy-pasteable: a couple of realistic
/// tools that lean on group/global defaults instead of repeating the per-tool
/// override knobs, and that showcase the modern parameter features (`type`,
/// `default`, `enum`, a flagged param) plus the `output = "json"` tool format.
pub fn output_generated_example_config() -> Result<()> {
    print!("{EXAMPLE_CONFIG_TOML}");
    Ok(())
}

/// A minimal, modern, copy-pasteable example config.
///
/// It leans on group defaults instead of repeating the per-tool override knobs,
/// and showcases the modern parameter features: `type`, `default`, `enum`,
/// numeric `minimum`, a flagged param, and a tool with `output = "json"`. Every
/// per-tool/per-parameter override is optional with a back-compat default, so
/// the example stays short. `cargo test` asserts this string parses.
const EXAMPLE_CONFIG_TOML: &str = r#"# gen-mcp example config. Generate with: gen-mcp config example > config.toml
#
# Tools are grouped; the group's `default_*` keys supply timeouts/output limits
# so individual tools stay clean. See `gen-mcp config schema` for every field.

# Advertise the runtime-override knobs (timeout, stop_after, output_*_lines,
# stderr_lines) in each tool's input schema. They are honored either way; this
# only controls whether clients see them. Defaults to false.
expose_runtime_overrides = false

[groups.files]
default_timeout = 30
default_timeout_max = 300
default_output_head_lines = 100
default_output_tail_lines = 100

  # A tool with a typed, defaulted, flagged parameter.
  [[groups.files.tools]]
  name = "head"
  description = "Print the first lines of a file"
  command = "/usr/bin/head"
  arg_order = ["lines", "file"]

    [groups.files.tools.parameters.lines]
    description = "Number of lines to print"
    type = "integer"      # string | integer | number | boolean | enum (default: inferred)
    default = 10
    minimum = 1
    flag = "-n"           # emitted as: -n <lines>
    takes_value = true

    [groups.files.tools.parameters.file]
    description = "Path to the file to read"
    required = true       # positional argument

  # A tool that returns structured JSON (parsed from stdout) and uses an enum.
  [[groups.files.tools]]
  name = "list"
  description = "List items in a known format"
  command = "/usr/bin/mylister"
  output = "json"         # text (default) | json
  arg_order = ["format"]

    [groups.files.tools.parameters.format]
    description = "Output format"
    type = "enum"
    enum = ["json", "yaml"]
    default = "json"
    flag = "--format"
    takes_value = true
"#;

/// Output Markdown documentation generated from the Rust config structures (stays in sync).
pub fn output_docs_generated() -> Result<()> {
    let root = schema_for!(crate::config::ConfigToml);
    let docs = render_markdown_docs_from_schema(&root);
    println!("{docs}");
    Ok(())
}

/// Output Markdown documentation for the configuration file format (hand-written).
pub fn output_docs_curated() -> Result<()> {
    let docs = r#"# gen-mcp Configuration Schema

## Overview

The gen-mcp configuration file uses TOML format and organizes tools into groups.

## Top-level keys

- `expose_runtime_overrides` (optional, boolean): Advertise the runtime-override
  knobs (`timeout`, `stop_after`, `output_head_lines`, `output_tail_lines`,
  `stderr_lines`) in every tool's input schema. Defaults to `false`. The
  overrides are always honored when a client sends them; this only controls
  whether they appear in the schema.

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
- `output` (optional): `text` (default — classic "Exit code / STDOUT / STDERR" block) or `json` (parse stdout as JSON on success and return it as `structuredContent`; falls back to text on a parse failure or non-zero exit)
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
- `type` (optional): JSON Schema type advertised to the client — `string`, `integer`, `number`, `boolean`, or `enum`. Defaults to inference (flag-only → `boolean`, otherwise `string`). Orthogonal to the CLI emission knobs below.
- `enum` (optional, list of strings): Allowed values; emits a JSON Schema `enum` constraint (use with `type = "enum"`)
- `default` (optional): Default value advertised in the schema
- `minimum` / `maximum` (optional, numbers): Numeric bounds advertised in the schema
- `example`: Example value (optional)
- `flag` (optional): Emit this CLI flag when the parameter is provided (e.g. `-r`, `-n`)
- `takes_value` (optional, boolean): If `true`, emit `flag` followed by the parameter value (e.g. `-n 50`)
- `required`: Whether parameter is required (default: false)
- `split_args` (optional, boolean): If `true`, split a positional parameter's value into multiple arguments using shell-style parsing (quotes, escapes, multi-line). Defaults to `false` (the value is passed through as a single argument). Only meaningful when `flag` is not set.

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

    out.push_str("# gen-mcp Configuration (generated)\n\n");
    out.push_str(
        "This documentation is generated from the Rust configuration structs (field doc comments + schema), so it stays in sync with the running binary.\n\n",
    );
    out.push_str("## Quick commands\n\n");
    out.push_str("- `gen-mcp config schema` (generated JSON Schema)\n");
    out.push_str("- `gen-mcp config example` (minimal example TOML)\n");
    out.push_str("- `gen-mcp config docs` (generated docs)\n");
    out.push_str("- `gen-mcp config docs --curated` (hand-written docs)\n\n");

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
        // New fields from the modernization are reflected in the schema.
        assert!(s.contains("expose_runtime_overrides"));
        assert!(s.contains("\"minimum\""));
        assert!(s.contains("\"maximum\""));
        assert!(s.contains("ParamType"));
        assert!(s.contains("OutputFormat"));
    }

    #[test]
    fn test_example_config_parses_and_uses_new_fields() {
        use crate::config::{OutputFormat, ParamType};

        // The published example must always parse.
        let config = crate::config::Config::from_str(EXAMPLE_CONFIG_TOML)
            .expect("example config must parse");
        assert!(!config.expose_runtime_overrides);

        let group = config.groups.get("files").expect("files group");
        let list = group
            .tools
            .iter()
            .find(|t| t.name == "list")
            .expect("list tool");
        assert_eq!(list.output, OutputFormat::Json);

        let head = group
            .tools
            .iter()
            .find(|t| t.name == "head")
            .expect("head tool");
        assert_eq!(head.parameters["lines"].param_type, ParamType::Integer);
        assert_eq!(head.parameters["lines"].minimum, Some(1.0));
        assert_eq!(
            list.parameters["format"].r#enum.as_deref(),
            Some(["json".to_string(), "yaml".to_string()].as_slice())
        );
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
