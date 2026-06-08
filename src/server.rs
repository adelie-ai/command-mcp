#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// MCP server implementation

use crate::config::Config;
use crate::error::{McpError, Result};
use crate::executor::{ExecutionResult, execute_command};
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export WebSocketAuth for external use
pub use crate::config::WebSocketAuth;

/// Parse shell-like arguments respecting quotes and escapes
/// Handles single quotes, double quotes, escaped characters, and multi-line content
#[cfg(test)]
pub(crate) fn parse_shell_args(input: &str) -> Result<Vec<String>> {
    parse_shell_args_impl(input)
}

#[cfg(not(test))]
fn parse_shell_args(input: &str) -> Result<Vec<String>> {
    parse_shell_args_impl(input)
}

fn parse_shell_args_impl(input: &str) -> Result<Vec<String>> {
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
        return Err(McpError::InvalidToolParameters(format!(
            "Unclosed quote in arguments: {}",
            input
        ))
        .into());
    }

    // Add final argument if any (skip empty)
    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    Ok(args)
}

/// MCP server state
pub struct McpServer {
    /// Tool registry
    tool_registry: Arc<ToolRegistry>,
    /// Server configuration
    config: Arc<Config>,
    /// Initialized flag
    initialized: Arc<RwLock<bool>>,
    /// WebSocket authentication configuration
    websocket_auth: Option<crate::config::WebSocketAuth>,
}

impl McpServer {
    /// Create a new MCP server from configuration
    pub fn new(config: Config) -> Result<Self> {
        let tool_registry = Arc::new(ToolRegistry::from_config(&config)?);
        let websocket_auth = config.websocket_auth.clone();
        let config = Arc::new(config);
        let initialized = Arc::new(RwLock::new(false));

        Ok(Self {
            tool_registry,
            config,
            initialized,
            websocket_auth,
        })
    }

    /// Get WebSocket authentication configuration
    pub fn websocket_auth(&self) -> Option<&crate::config::WebSocketAuth> {
        self.websocket_auth.as_ref()
    }

    /// Handle initialize request
    pub async fn handle_initialize(
        &self,
        protocol_version: &str,
        _client_capabilities: &Value,
    ) -> Result<Value> {
        // Validate protocol version
        if protocol_version != "2024-11-05"
            && protocol_version != "2025-06-18"
            && protocol_version != "2025-11-25"
        {
            return Err(McpError::InvalidProtocolVersion(protocol_version.to_string()).into());
        }

        // Generate tool schemas
        let mut tools = Vec::new();
        for tool in self.tool_registry.all_tools() {
            let schema = self.tool_registry.generate_tool_schema(tool);
            tools.push(serde_json::json!({
                "name": schema.name,
                "description": schema.description,
                "inputSchema": schema.input_schema,
            }));
        }

        // Build server capabilities
        let capabilities = serde_json::json!({
            "protocolVersion": protocol_version,
            "serverInfo": {
                "name": "genmcp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "tools": {
                    "listChanged": false,
                },
            },
            "tools": tools,
        });

        Ok(capabilities)
    }

    /// Handle initialized notification
    pub async fn handle_initialized(&self) -> Result<()> {
        let mut initialized = self.initialized.write().await;
        *initialized = true;
        Ok(())
    }

    /// Handle tool call
    pub async fn handle_tool_call(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<ExecutionResult> {
        // Get tool from registry
        let tool = self
            .tool_registry
            .get_tool(tool_name)
            .ok_or_else(|| McpError::ToolNotFound(tool_name.to_string()))?;

        // Parse arguments
        let args_map = arguments.as_object().ok_or_else(|| {
            McpError::InvalidToolParameters("Arguments must be an object".to_string())
        })?;

        // Extract tool-specific parameters in deterministic, CLI-natural order.
        // This avoids surprising behavior from HashMap iteration order.
        let mut tool_args: Vec<String> = Vec::new();

        let mut emit_param = |param_name: &str| -> Result<()> {
            let param = tool.parameters.get(param_name).ok_or_else(|| {
                McpError::InvalidToolParameters(format!(
                    "Tool configuration error: arg_order references unknown parameter: {}",
                    param_name
                ))
            })?;

            let value = args_map.get(param_name);

            if value.is_none() {
                if param.required {
                    return Err(McpError::InvalidToolParameters(format!(
                        "Missing required parameter: {}",
                        param_name
                    ))
                    .into());
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
                    let parsed_args = parse_shell_args(str_value)?;
                    tool_args.extend(parsed_args);
                } else {
                    tool_args.push(str_value.to_string());
                }
            } else {
                tool_args.push(value.to_string());
            }

            Ok(())
        };

        // Emit args in configured order first
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

        // Extract runtime overrides
        let timeout = args_map.get("timeout").and_then(|v| v.as_u64());
        let stop_after = args_map.get("stop_after").and_then(|v| v.as_u64());
        let output_head_lines = args_map.get("output_head_lines").and_then(|v| v.as_u64());
        let output_tail_lines = args_map.get("output_tail_lines").and_then(|v| v.as_u64());
        let stderr_lines = args_map.get("stderr_lines").and_then(|v| v.as_u64());

        // Validate runtime overrides
        self.tool_registry.validate_runtime_overrides(
            tool,
            timeout,
            stop_after,
            output_head_lines,
            output_tail_lines,
            stderr_lines,
        )?;

        // Use overrides or defaults
        let timeout_secs = timeout.unwrap_or(tool.timeout);
        let stop_after_secs = stop_after.or(if tool.stop_after > 0 {
            Some(tool.stop_after)
        } else {
            None
        });
        let output_head = output_head_lines.unwrap_or(tool.output_head_lines);
        let output_tail = output_tail_lines.unwrap_or(tool.output_tail_lines);
        let stderr = stderr_lines.unwrap_or(tool.stderr_lines);

        // Execute command
        execute_command(
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
    }

    /// Handle shutdown request
    pub async fn handle_shutdown(&self) -> Result<()> {
        let mut initialized = self.initialized.write().await;
        *initialized = false;
        Ok(())
    }

    /// List tools in MCP schema format
    pub fn list_tools(&self) -> Value {
        let mut tools = Vec::new();
        for tool in self.tool_registry.all_tools() {
            let schema = self.tool_registry.generate_tool_schema(tool);
            tools.push(serde_json::json!({
                "name": schema.name,
                "description": schema.description,
                "inputSchema": schema.input_schema,
            }));
        }
        serde_json::Value::Array(tools)
    }

    /// Check if server is initialized
    pub async fn is_initialized(&self) -> bool {
        *self.initialized.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn create_test_config() -> Config {
        let toml = r#"
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
"#;
        Config::from_str(toml).unwrap()
    }

    #[tokio::test]
    async fn test_server_creation() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        assert!(!server.is_initialized().await);
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let capabilities = server
            .handle_initialize("2024-11-05", &serde_json::json!({}))
            .await
            .unwrap();

        assert!(capabilities.get("protocolVersion").is_some());
        assert!(capabilities.get("serverInfo").is_some());
        assert!(capabilities.get("tools").is_some());

        let tools = capabilities.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0].get("name").unwrap().as_str().unwrap(),
            "test_group_echo"
        );
    }

    #[tokio::test]
    async fn test_handle_initialize_2025_11_25() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let capabilities = server
            .handle_initialize("2025-11-25", &serde_json::json!({}))
            .await
            .unwrap();

        assert!(capabilities.get("protocolVersion").is_some());
        assert_eq!(
            capabilities
                .get("protocolVersion")
                .unwrap()
                .as_str()
                .unwrap(),
            "2025-11-25"
        );
    }

    #[tokio::test]
    async fn test_handle_initialize_invalid_version() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_initialize("invalid-version", &serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_initialized() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        assert!(!server.is_initialized().await);
        server.handle_initialized().await.unwrap();
        assert!(server.is_initialized().await);
    }

    #[tokio::test]
    async fn test_handle_shutdown() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        server.handle_initialized().await.unwrap();
        assert!(server.is_initialized().await);

        server.handle_shutdown().await.unwrap();
        assert!(!server.is_initialized().await);
    }

    #[tokio::test]
    async fn test_handle_tool_call_success() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call(
                "test_group_echo",
                &serde_json::json!({
                    "text": "hello"
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_handle_tool_call_not_found() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call("nonexistent_tool", &serde_json::json!({}))
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Mcp(crate::error::McpError::ToolNotFound(_)) => {}
                _ => panic!("Expected ToolNotFound error"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_tool_call_missing_required_param() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call("test_group_echo", &serde_json::json!({}))
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Mcp(crate::error::McpError::InvalidToolParameters(
                    _,
                )) => {}
                _ => panic!("Expected InvalidToolParameters error"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_tool_call_override_exceeds_max() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call(
                "test_group_echo",
                &serde_json::json!({
                    "text": "hello",
                    "timeout": 500  // Exceeds max of 300
                }),
            )
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Mcp(crate::error::McpError::OverrideExceedsMax {
                    ..
                }) => {}
                _ => panic!("Expected OverrideExceedsMax error"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_tool_call_with_runtime_overrides() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call(
                "test_group_echo",
                &serde_json::json!({
                    "text": "hello",
                    "timeout": 100,  // Within max
                    "output_head_lines": 50,
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn test_parse_shell_args_simple() {
        let args = parse_shell_args("hello world").unwrap();
        assert_eq!(args, vec!["hello", "world"]);
    }

    #[test]
    fn test_parse_shell_args_single_quotes() {
        let args = parse_shell_args("hello 'world with spaces'").unwrap();
        assert_eq!(args, vec!["hello", "world with spaces"]);
    }

    #[test]
    fn test_parse_shell_args_double_quotes() {
        let args = parse_shell_args(r#"hello "world with spaces""#).unwrap();
        assert_eq!(args, vec!["hello", "world with spaces"]);
    }

    #[test]
    fn test_parse_shell_args_mixed_quotes() {
        let args = parse_shell_args(r#"hello 'single' "double" test"#).unwrap();
        assert_eq!(args, vec!["hello", "single", "double", "test"]);
    }

    #[test]
    fn test_parse_shell_args_escaped_chars() {
        let args = parse_shell_args(r#"hello\ world test"#).unwrap();
        assert_eq!(args, vec!["hello world", "test"]);
    }

    #[test]
    fn test_parse_shell_args_escaped_quotes() {
        let args = parse_shell_args(r#"hello \"world\" test"#).unwrap();
        assert_eq!(args, vec!["hello", "\"world\"", "test"]);
    }

    #[test]
    fn test_parse_shell_args_multiline() {
        // Newlines are whitespace, so should split into separate arguments
        let multiline = "hello\nworld\ntest";
        let args = parse_shell_args(multiline).unwrap();
        assert_eq!(args, vec!["hello", "world", "test"]);
    }

    #[test]
    fn test_parse_shell_args_heredoc_like() {
        // Heredoc-like with spaces should split
        let heredoc = "cat <<EOF\nhello world\nEOF";
        let args = parse_shell_args(heredoc).unwrap();
        // Should split on whitespace (including newlines)
        assert!(args.len() > 1);
        assert!(args.iter().any(|a| a.contains("<<EOF")));
    }

    #[test]
    fn test_parse_shell_args_quoted_multiline() {
        let multiline = r#"hello "world
with
newlines" test"#;
        let args = parse_shell_args(multiline).unwrap();
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], "hello");
        assert_eq!(args[1], "world\nwith\nnewlines");
        assert_eq!(args[2], "test");
    }

    #[test]
    fn test_parse_shell_args_single_quote_preserves_backslash() {
        // In single quotes, backslash is literal
        let args = parse_shell_args(r#"hello 'world\test' foo"#).unwrap();
        assert_eq!(args, vec!["hello", "world\\test", "foo"]);
    }

    #[test]
    fn test_parse_shell_args_double_quote_escapes() {
        // In double quotes, backslash escapes
        let args = parse_shell_args(r#"hello "world\ttest" foo"#).unwrap();
        assert_eq!(args, vec!["hello", "world\ttest", "foo"]);
    }

    #[test]
    fn test_parse_shell_args_unclosed_quote() {
        let result = parse_shell_args(r#"hello "world"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_shell_args_empty() {
        let args = parse_shell_args("").unwrap();
        assert_eq!(args, Vec::<String>::new());
    }

    #[test]
    fn test_parse_shell_args_whitespace_only() {
        let args = parse_shell_args("   ").unwrap();
        assert_eq!(args, Vec::<String>::new());
    }

    #[test]
    fn test_parse_shell_args_docker_like() {
        // Simulate docker run arguments
        let docker_args = r#"run --name my-container -p 8080:80 -d nginx:latest"#;
        let args = parse_shell_args(docker_args).unwrap();
        assert_eq!(
            args,
            vec![
                "run",
                "--name",
                "my-container",
                "-p",
                "8080:80",
                "-d",
                "nginx:latest"
            ]
        );
    }

    #[test]
    fn test_parse_shell_args_docker_with_quotes() {
        // Docker args with quoted values
        let docker_args = r#"run --name "my container" -e "NODE_ENV=production" nginx:latest"#;
        let args = parse_shell_args(docker_args).unwrap();
        assert_eq!(
            args,
            vec![
                "run",
                "--name",
                "my container",
                "-e",
                "NODE_ENV=production",
                "nginx:latest"
            ]
        );
    }

    #[tokio::test]
    async fn test_bash_command_with_heredoc() {
        // Test that bash -c with heredoc works correctly
        let config_toml = r#"
[groups.test]
  [[groups.test.tools]]
  name = "bash"
  description = "Bash command"
  command = "/bin/bash"
  arg_order = ["command"]

  [groups.test.tools.parameters.command]
  description = "Command"
  required = true
  flag = "-c"
  takes_value = true
"#;
        let config = Config::from_str(config_toml).unwrap();
        let server = McpServer::new(config).unwrap();

        // Test with a heredoc-like command
        let heredoc_cmd = r#"cat <<EOF
hello world
this is a test
EOF"#;
        let result = server
            .handle_tool_call(
                "test_bash",
                &serde_json::json!({
                    "command": heredoc_cmd
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
        assert!(result.stdout.contains("this is a test"));
    }

    #[tokio::test]
    async fn test_bash_command_with_quoted_spaces() {
        // Test that bash -c with quoted strings works correctly
        let config_toml = r#"
[groups.test]
  [[groups.test.tools]]
  name = "bash"
  description = "Bash command"
  command = "/bin/bash"
  arg_order = ["command"]

  [groups.test.tools.parameters.command]
  description = "Command"
  required = true
  flag = "-c"
  takes_value = true
"#;
        let config = Config::from_str(config_toml).unwrap();
        let server = McpServer::new(config).unwrap();

        // Test with quoted string containing spaces
        let quoted_cmd = r#"echo "hello world with spaces""#;
        let result = server
            .handle_tool_call(
                "test_bash",
                &serde_json::json!({
                    "command": quoted_cmd
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world with spaces"));
    }

    #[tokio::test]
    async fn test_args_param_without_split_is_passed_verbatim() {
        // A parameter literally named "args" must NOT be shell-split unless
        // split_args is set. It should be passed through as a single argument.
        let config_toml = r#"
[groups.test]
  [[groups.test.tools]]
  name = "echo"
  description = "Echo"
  command = "/bin/echo"
  arg_order = ["args"]

  [groups.test.tools.parameters.args]
  description = "Args"
  required = true
"#;
        let config = Config::from_str(config_toml).unwrap();
        let server = McpServer::new(config).unwrap();

        // Two space-separated words: as a single argument echo prints them with
        // a single space and no splitting side effects.
        let result = server
            .handle_tool_call("test_echo", &serde_json::json!({ "args": "alpha beta" }))
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "alpha beta");
    }

    #[tokio::test]
    async fn test_split_args_opt_in_splits_value() {
        // With split_args = true on a positional parameter (any name), the value
        // is parsed shell-style into multiple arguments. We use `printf '%s\n'`
        // so each resulting argument lands on its own output line.
        let config_toml = r#"
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
"#;
        let config = Config::from_str(config_toml).unwrap();
        let server = McpServer::new(config).unwrap();

        let result = server
            .handle_tool_call(
                "test_printf",
                &serde_json::json!({
                    "fmt": "%s\n",
                    "items": r#"one "two three" four"#,
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        // Splitting yields three args: one / two three / four.
        assert_eq!(lines, vec!["one", "two three", "four"]);
    }
}
