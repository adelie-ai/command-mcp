#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// MCP server implementation

use crate::config::Config;
use crate::error::{McpError, Result};
use crate::executor::{execute_command, ExecutionResult};
use crate::tools::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export WebSocketAuth for external use
pub use crate::config::WebSocketAuth;

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
        if protocol_version != "2024-11-05" && protocol_version != "2025-06-18" && protocol_version != "2025-11-25" {
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
        let tool = self.tool_registry.get_tool(tool_name)
            .ok_or_else(|| McpError::ToolNotFound(tool_name.to_string()))?;

        // Parse arguments
        let args_map = arguments.as_object()
            .ok_or_else(|| McpError::InvalidToolParameters("Arguments must be an object".to_string()))?;

        // Extract tool-specific parameters
        let mut tool_args = Vec::new();
        for (param_name, param) in &tool.parameters {
            if let Some(value) = args_map.get(param_name) {
                if let Some(str_value) = value.as_str() {
                    // Special handling for "args" parameter: split by spaces for commands like docker
                    // This allows passing "run --name my-container nginx" as a single string
                    if param_name == "args" && str_value.contains(' ') {
                        // Split by spaces, but preserve quoted strings
                        // Simple implementation: split by spaces, handle basic quoting
                        let parts: Vec<&str> = str_value.split_whitespace().collect();
                        for part in parts {
                            // Remove surrounding quotes if present
                            let cleaned = part.trim_matches('"').trim_matches('\'');
                            if !cleaned.is_empty() {
                                tool_args.push(cleaned.to_string());
                            }
                        }
                    } else {
                        tool_args.push(str_value.to_string());
                    }
                } else {
                    tool_args.push(value.to_string());
                }
            } else if param.required {
                return Err(McpError::InvalidToolParameters(
                    format!("Missing required parameter: {}", param_name)
                ).into());
            }
        }

        // Extract runtime overrides
        let timeout = args_map.get("timeout")
            .and_then(|v| v.as_u64());
        let stop_after = args_map.get("stop_after")
            .and_then(|v| v.as_u64());
        let output_head_lines = args_map.get("output_head_lines")
            .and_then(|v| v.as_u64());
        let output_tail_lines = args_map.get("output_tail_lines")
            .and_then(|v| v.as_u64());
        let stderr_lines = args_map.get("stderr_lines")
            .and_then(|v| v.as_u64());

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
        let stop_after_secs = stop_after.or(if tool.stop_after > 0 { Some(tool.stop_after) } else { None });
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
        ).await
    }

    /// Handle shutdown request
    pub async fn handle_shutdown(&self) -> Result<()> {
        let mut initialized = self.initialized.write().await;
        *initialized = false;
        Ok(())
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
        
        let capabilities = server.handle_initialize(
            "2024-11-05",
            &serde_json::json!({}),
        ).await.unwrap();
        
        assert!(capabilities.get("protocolVersion").is_some());
        assert!(capabilities.get("serverInfo").is_some());
        assert!(capabilities.get("tools").is_some());
        
        let tools = capabilities.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].get("name").unwrap().as_str().unwrap(), "test_group_echo");
    }

    #[tokio::test]
    async fn test_handle_initialize_2025_11_25() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        
        let capabilities = server.handle_initialize(
            "2025-11-25",
            &serde_json::json!({}),
        ).await.unwrap();
        
        assert!(capabilities.get("protocolVersion").is_some());
        assert_eq!(capabilities.get("protocolVersion").unwrap().as_str().unwrap(), "2025-11-25");
    }

    #[tokio::test]
    async fn test_handle_initialize_invalid_version() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        
        let result = server.handle_initialize(
            "invalid-version",
            &serde_json::json!({}),
        ).await;
        
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
        
        let result = server.handle_tool_call(
            "test_group_echo",
            &serde_json::json!({
                "text": "hello"
            }),
        ).await.unwrap();
        
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_handle_tool_call_not_found() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        
        let result = server.handle_tool_call(
            "nonexistent_tool",
            &serde_json::json!({}),
        ).await;
        
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
        
        let result = server.handle_tool_call(
            "test_group_echo",
            &serde_json::json!({}),
        ).await;
        
        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Mcp(crate::error::McpError::InvalidToolParameters(_)) => {}
                _ => panic!("Expected InvalidToolParameters error"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_tool_call_override_exceeds_max() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        
        let result = server.handle_tool_call(
            "test_group_echo",
            &serde_json::json!({
                "text": "hello",
                "timeout": 500  // Exceeds max of 300
            }),
        ).await;
        
        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Mcp(crate::error::McpError::OverrideExceedsMax { .. }) => {}
                _ => panic!("Expected OverrideExceedsMax error"),
            }
        }
    }

    #[tokio::test]
    async fn test_handle_tool_call_with_runtime_overrides() {
        let config = create_test_config();
        let server = McpServer::new(config).unwrap();
        
        let result = server.handle_tool_call(
            "test_group_echo",
            &serde_json::json!({
                "text": "hello",
                "timeout": 100,  // Within max
                "output_head_lines": 50,
            }),
        ).await.unwrap();
        
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }
}
