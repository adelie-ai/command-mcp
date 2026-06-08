#![deny(warnings)]

//! The [`mcp_core::McpService`] implementation for gen-mcp.
//!
//! gen-mcp's tool set is built *dynamically* from a TOML config: every
//! configured `{group}_{tool}` command becomes one MCP tool whose input schema
//! is generated from the tool's declared parameters (plus the standard runtime
//! override knobs). This module owns the loaded [`Config`] / [`ToolRegistry`]
//! and translates an incoming `tools/call` into a CLI invocation via the
//! executor — preserving deterministic argument ordering, the opt-in
//! `split_args` shell-splitting, runtime-override validation, and the
//! "Exit code / STDOUT / STDERR" result formatting the old server produced.

use crate::config::{Config, OutputFormat};
use crate::error::McpError;
use crate::executor::execute_command;
use crate::tools::ToolRegistry;
use mcp_core::{CallError, McpService, ToolDef, ToolReply, async_trait};
use serde_json::Value;
use std::sync::Arc;

/// Parse shell-like arguments respecting quotes and escapes.
/// Handles single quotes, double quotes, escaped characters, and multi-line
/// content. Used for positional parameters with `split_args = true`.
pub(crate) fn parse_shell_args(input: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut chars = input.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            // Handle escaped character
            if in_double_quote {
                // In double quotes, handle escape sequences
                match ch {
                    't' => current_arg.push('\t'),
                    'n' => current_arg.push('\n'),
                    'r' => current_arg.push('\r'),
                    '\\' => current_arg.push('\\'),
                    '"' => current_arg.push('"'),
                    _ => current_arg.push(ch),
                }
            } else {
                // Outside quotes, escaped character is literal
                current_arg.push(ch);
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quote => {
                // Escape next character (unless in single quotes where backslash is literal)
                escaped = true;
            }
            '\'' if !in_double_quote && !escaped => {
                // Toggle single quote mode
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote && !escaped => {
                // Toggle double quote mode
                in_double_quote = !in_double_quote;
            }
            '\\' if in_single_quote => {
                // In single quotes, backslash is literal
                current_arg.push(ch);
            }
            c if c.is_whitespace() && !in_single_quote && !in_double_quote => {
                // Whitespace outside quotes: end current argument
                if !current_arg.is_empty() {
                    args.push(current_arg);
                    current_arg = String::new();
                }
                // Skip remaining whitespace
                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_whitespace() {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            _ => {
                // Regular character: add to current argument
                current_arg.push(ch);
            }
        }
    }

    // Check for unclosed quotes
    if in_single_quote || in_double_quote {
        return Err(format!("Unclosed quote in arguments: {}", input));
    }

    // Add final argument if any (skip empty)
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    Ok(args)
}

/// The gen-mcp MCP service: a tool registry built from the loaded TOML config.
/// The tool set is static for a given config (it does not change at runtime).
pub struct GenMcpService {
    tool_registry: Arc<ToolRegistry>,
}

impl GenMcpService {
    /// Build the service from a parsed [`Config`].
    pub fn new(config: Config) -> crate::error::Result<Self> {
        let tool_registry = Arc::new(ToolRegistry::from_config(&config)?);
        Ok(Self { tool_registry })
    }
}

#[async_trait]
impl McpService for GenMcpService {
    /// The dynamically generated tool list: one [`ToolDef`] per configured
    /// command, with its generated input schema (parameter typing + runtime
    /// override knobs).
    fn tools(&self) -> Vec<ToolDef> {
        self.tool_registry
            .all_tools()
            .map(|tool| {
                let schema = self.tool_registry.generate_tool_schema(tool);
                ToolDef::new(schema.name, schema.description, schema.input_schema)
            })
            .collect()
    }

    /// Look up the configured command by name and run it via the executor.
    async fn call_tool(&self, name: &str, arguments: &Value) -> Result<ToolReply, CallError> {
        // Unknown tool → tool-level error (isError content the model can react to).
        let tool = self
            .tool_registry
            .get_tool(name)
            .ok_or_else(|| CallError::tool(format!("Tool not found: {name}")))?;

        // Arguments may be omitted entirely (Null); treat that as an empty object.
        let empty = serde_json::Map::new();
        let args_map = match arguments {
            Value::Null => &empty,
            Value::Object(map) => map,
            _ => {
                return Err(CallError::invalid_params(
                    "Arguments must be an object".to_string(),
                ));
            }
        };

        // Build CLI args in deterministic, CLI-natural order (arg_order first,
        // then any remaining parameters sorted). Mirrors the previous server.
        let mut tool_args: Vec<String> = Vec::new();

        let mut emit_param = |param_name: &str| -> Result<(), CallError> {
            let param = tool.parameters.get(param_name).ok_or_else(|| {
                CallError::invalid_params(format!(
                    "Tool configuration error: arg_order references unknown parameter: {}",
                    param_name
                ))
            })?;

            let value = args_map.get(param_name);

            if value.is_none() {
                if param.required {
                    return Err(CallError::invalid_params(format!(
                        "Missing required parameter: {}",
                        param_name
                    )));
                }
                return Ok(());
            }

            let value = value.unwrap();

            // Flagged parameters
            if let Some(flag) = &param.flag {
                if param.takes_value {
                    // Always emit flag + value when provided
                    tool_args.push(flag.clone());
                    if let Some(str_value) = value.as_str() {
                        tool_args.push(str_value.to_string());
                    } else {
                        tool_args.push(value.to_string());
                    }
                    return Ok(());
                }

                // Flag-only: emit when "truthy"
                let truthy = match value {
                    Value::Bool(b) => *b,
                    Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
                    Value::String(s) => {
                        let s = s.trim().to_ascii_lowercase();
                        !(s.is_empty() || s == "false" || s == "0" || s == "no" || s == "off")
                    }
                    _ => true,
                };

                if truthy {
                    tool_args.push(flag.clone());
                }

                return Ok(());
            }

            // Positional parameters
            if let Some(str_value) = value.as_str() {
                // Opt-in shell-style splitting: when `split_args = true`, parse
                // the value into multiple arguments honouring quotes, escapes,
                // and multi-line content. Otherwise pass the entire value as a
                // single argument, preserving multi-line content, heredocs, and
                // special characters.
                if param.split_args {
                    let parsed_args =
                        parse_shell_args(str_value).map_err(CallError::invalid_params)?;
                    tool_args.extend(parsed_args);
                } else {
                    tool_args.push(str_value.to_string());
                }
            } else {
                tool_args.push(value.to_string());
            }

            Ok(())
        };

        // Emit args in configured order first.
        let mut emitted = std::collections::HashSet::<String>::new();
        for param_name in &tool.arg_order {
            emitted.insert(param_name.clone());
            emit_param(param_name)?;
        }

        // Back-compat: append any remaining parameters deterministically.
        let mut remaining: Vec<String> = tool
            .parameters
            .keys()
            .filter(|k| !emitted.contains(*k))
            .cloned()
            .collect();
        remaining.sort();
        for param_name in remaining {
            emit_param(&param_name)?;
        }

        // Extract runtime overrides.
        let timeout = args_map.get("timeout").and_then(|v| v.as_u64());
        let stop_after = args_map.get("stop_after").and_then(|v| v.as_u64());
        let output_head_lines = args_map.get("output_head_lines").and_then(|v| v.as_u64());
        let output_tail_lines = args_map.get("output_tail_lines").and_then(|v| v.as_u64());
        let stderr_lines = args_map.get("stderr_lines").and_then(|v| v.as_u64());

        // Validate runtime overrides against the configured MAX values.
        self.tool_registry
            .validate_runtime_overrides(
                tool,
                timeout,
                stop_after,
                output_head_lines,
                output_tail_lines,
                stderr_lines,
            )
            .map_err(|e| match e {
                // OverrideExceedsMax is a bad-input condition the model should see.
                crate::error::GenMcpError::Mcp(McpError::OverrideExceedsMax { .. }) => {
                    CallError::invalid_params(e.to_string())
                }
                other => CallError::internal(other.to_string()),
            })?;

        // Use overrides or defaults.
        let timeout_secs = timeout.unwrap_or(tool.timeout);
        let stop_after_secs = stop_after.or(if tool.stop_after > 0 {
            Some(tool.stop_after)
        } else {
            None
        });
        let output_head = output_head_lines.unwrap_or(tool.output_head_lines);
        let output_tail = output_tail_lines.unwrap_or(tool.output_tail_lines);
        let stderr = stderr_lines.unwrap_or(tool.stderr_lines);

        // Execute the command.
        let exec_result = execute_command(
            &tool.command,
            &tool_args,
            timeout_secs,
            stop_after_secs,
            tool.termination_signal,
            tool.termination_grace_period,
            output_head,
            output_tail,
            stderr,
        )
        .await
        .map_err(|e| CallError::tool(e.to_string()))?;

        // A non-zero exit code is a tool-level error, unless the process was
        // stopped by the `stop_after` budget (which is a successful outcome).
        let is_error = exec_result.exit_code != 0 && !exec_result.stopped_after;

        // JSON output mode: when configured and the command succeeded, try to
        // parse stdout as JSON and return it as structured content. On a parse
        // failure (or a failed command) we fall back to the classic text block.
        if tool.output == OutputFormat::Json
            && !is_error
            && let Ok(parsed) = serde_json::from_str::<Value>(&exec_result.stdout)
        {
            // ToolReply::json puts the value in both a text block and
            // `structuredContent`. is_error is false here by construction.
            return ToolReply::json(&parsed).map_err(|e| CallError::internal(e.to_string()));
        }

        // Default: format the result exactly as the previous server did — a
        // single text block with the exit code, STDOUT, and STDERR (always
        // shown).
        let mut response_text = format!(
            "Exit code: {}\n\nSTDOUT:\n{}",
            exec_result.exit_code, exec_result.stdout
        );
        if exec_result.stderr.is_empty() {
            response_text.push_str("\n\nSTDERR:\n(no output)");
        } else {
            response_text.push_str(&format!("\n\nSTDERR:\n{}", exec_result.stderr));
        }

        let mut reply = ToolReply::text(response_text);
        reply.is_error = is_error;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn service_from(toml: &str) -> GenMcpService {
        GenMcpService::new(Config::from_str(toml).unwrap()).unwrap()
    }

    fn echo_service() -> GenMcpService {
        service_from(
            r#"
[groups.test_group]
default_timeout = 30
default_timeout_max = 300

  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"

    [groups.test_group.tools.parameters.text]
    description = "Text to echo"
    required = true
"#,
        )
    }

    #[test]
    fn tools_are_generated_from_config() {
        let svc = echo_service();
        let tools = svc.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_group_echo");
        // Declared parameters are present; the runtime-override knobs are NOT
        // advertised by default (only when `expose_runtime_overrides = true`).
        let props = tools[0].input_schema.get("properties").unwrap();
        assert!(props.get("text").is_some());
        assert!(props.get("timeout").is_none());
        assert!(props.get("stop_after").is_none());
    }

    #[test]
    fn tools_advertise_overrides_when_exposed() {
        let svc = service_from(
            r#"
expose_runtime_overrides = true

[groups.test_group]
default_timeout = 30
default_timeout_max = 300

  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"

    [groups.test_group.tools.parameters.text]
    description = "Text to echo"
    required = true
"#,
        );
        let tools = svc.tools();
        let props = tools[0].input_schema.get("properties").unwrap();
        assert!(props.get("timeout").is_some());
        assert!(props.get("stop_after").is_some());
    }

    #[tokio::test]
    async fn runtime_override_honored_even_when_not_advertised() {
        // The override is not in the schema but is still honored / validated at
        // call time: an over-max value is rejected.
        let svc = echo_service();
        let err = svc
            .call_tool(
                "test_group_echo",
                &serde_json::json!({ "text": "hi", "timeout": 99999 }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CallError::InvalidParams(_)));
    }

    #[tokio::test]
    async fn call_tool_success_formats_result() {
        let svc = echo_service();
        let reply = svc
            .call_tool("test_group_echo", &serde_json::json!({ "text": "hello" }))
            .await
            .unwrap();
        assert!(!reply.is_error);
        let text = match &reply.content[0] {
            mcp_core::Content::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        assert!(text.contains("Exit code: 0"));
        assert!(text.contains("hello"));
        assert!(text.contains("STDERR:"));
    }

    #[tokio::test]
    async fn call_tool_unknown_is_tool_error() {
        let svc = echo_service();
        let err = svc
            .call_tool("nope", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, CallError::Tool(_)));
    }

    #[tokio::test]
    async fn call_tool_missing_required_is_invalid_params() {
        let svc = echo_service();
        let err = svc
            .call_tool("test_group_echo", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, CallError::InvalidParams(_)));
    }

    #[tokio::test]
    async fn call_tool_override_exceeds_max_is_invalid_params() {
        let svc = echo_service();
        let err = svc
            .call_tool(
                "test_group_echo",
                &serde_json::json!({ "text": "hi", "timeout": 500 }),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, CallError::InvalidParams(_)));
    }

    #[tokio::test]
    async fn call_tool_nonzero_exit_is_tool_error() {
        let svc = service_from(
            r#"
[groups.g]
  [[groups.g.tools]]
  name = "false"
  description = "Always fails"
  command = "/bin/false"
"#,
        );
        let reply = svc
            .call_tool("g_false", &serde_json::json!({}))
            .await
            .unwrap();
        assert!(reply.is_error);
    }

    #[tokio::test]
    async fn json_output_yields_structured_content() {
        // A tool with `output = "json"` whose stdout is valid JSON returns
        // non-error structured content (and a text block).
        let svc = service_from(
            r#"
[groups.g]
  [[groups.g.tools]]
  name = "obj"
  description = "emit json"
  command = "/bin/echo"
  output = "json"
  arg_order = ["payload"]

    [groups.g.tools.parameters.payload]
    description = "json payload"
    required = true
"#,
        );
        let reply = svc
            .call_tool("g_obj", &serde_json::json!({ "payload": r#"{"k":1}"# }))
            .await
            .unwrap();
        assert!(!reply.is_error);
        let sc = reply
            .structured_content
            .as_ref()
            .expect("expected structuredContent");
        assert_eq!(sc.get("k").unwrap(), 1);
    }

    #[tokio::test]
    async fn json_output_invalid_json_falls_back_to_text() {
        // `output = "json"` but stdout is not valid JSON → classic text block,
        // no structured content.
        let svc = service_from(
            r#"
[groups.g]
  [[groups.g.tools]]
  name = "notjson"
  description = "emit text"
  command = "/bin/echo"
  output = "json"
  arg_order = ["payload"]

    [groups.g.tools.parameters.payload]
    description = "payload"
    required = true
"#,
        );
        let reply = svc
            .call_tool("g_notjson", &serde_json::json!({ "payload": "not json" }))
            .await
            .unwrap();
        assert!(!reply.is_error);
        assert!(reply.structured_content.is_none());
        let text = match &reply.content[0] {
            mcp_core::Content::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        assert!(text.contains("Exit code: 0"));
        assert!(text.contains("not json"));
    }

    #[tokio::test]
    async fn args_param_without_split_is_passed_verbatim() {
        let svc = service_from(
            r#"
[groups.test]
  [[groups.test.tools]]
  name = "echo"
  description = "Echo"
  command = "/bin/echo"
  arg_order = ["args"]

  [groups.test.tools.parameters.args]
  description = "Args"
  required = true
"#,
        );
        let reply = svc
            .call_tool("test_echo", &serde_json::json!({ "args": "alpha beta" }))
            .await
            .unwrap();
        let text = match &reply.content[0] {
            mcp_core::Content::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        // Echoed as a single argument: "alpha beta" on the STDOUT line.
        assert!(text.contains("alpha beta"));
    }

    #[tokio::test]
    async fn split_args_opt_in_splits_value() {
        let svc = service_from(
            r#"
[groups.test]
  [[groups.test.tools]]
  name = "printf"
  description = "printf each arg on a line"
  command = "/usr/bin/printf"
  arg_order = ["fmt", "items"]

  [groups.test.tools.parameters.fmt]
  description = "format"
  required = true

  [groups.test.tools.parameters.items]
  description = "items"
  required = true
  split_args = true
"#,
        );
        let reply = svc
            .call_tool(
                "test_printf",
                &serde_json::json!({
                    "fmt": "%s\n",
                    "items": r#"one "two three" four"#,
                }),
            )
            .await
            .unwrap();
        let text = match &reply.content[0] {
            mcp_core::Content::Text(t) => t.clone(),
            _ => panic!("expected text content"),
        };
        assert!(text.contains("one"));
        assert!(text.contains("two three"));
        assert!(text.contains("four"));
    }

    #[test]
    fn parse_shell_args_simple() {
        assert_eq!(
            parse_shell_args("hello world").unwrap(),
            vec!["hello", "world"]
        );
    }

    #[test]
    fn parse_shell_args_quotes() {
        assert_eq!(
            parse_shell_args(r#"hello 'world with spaces'"#).unwrap(),
            vec!["hello", "world with spaces"]
        );
        assert_eq!(
            parse_shell_args(r#"hello "world with spaces""#).unwrap(),
            vec!["hello", "world with spaces"]
        );
    }

    #[test]
    fn parse_shell_args_unclosed_quote_errors() {
        assert!(parse_shell_args(r#"hello "world"#).is_err());
    }

    #[test]
    fn parse_shell_args_empty() {
        assert_eq!(parse_shell_args("").unwrap(), Vec::<String>::new());
    }
}
