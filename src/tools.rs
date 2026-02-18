#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Tool registry and MCP tool definitions

use crate::config::{Config, ResolvedTool};
use crate::error::{Result, ToolRegistryError};
use std::collections::HashMap;

/// Tool registry that manages all available tools
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    /// Map of full tool name to resolved tool configuration
    tools: HashMap<String, ResolvedTool>,
}

impl ToolRegistry {
    /// Create a new tool registry from a configuration
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut tools = HashMap::new();
        let resolved_tools = config.get_all_tools()?;

        for tool in resolved_tools {
            let full_name = tool.full_name.clone();
            if tools.contains_key(&full_name) {
                return Err(ToolRegistryError::DuplicateTool(full_name).into());
            }
            tools.insert(full_name, tool);
        }

        Ok(ToolRegistry { tools })
    }

    /// Get a tool by its full name
    pub fn get_tool(&self, name: &str) -> Option<&ResolvedTool> {
        self.tools.get(name)
    }

    /// Get all tools
    pub fn all_tools(&self) -> impl Iterator<Item = &ResolvedTool> {
        self.tools.values()
    }

    /// Generate MCP tool schema for a tool
    pub fn generate_tool_schema(&self, tool: &ResolvedTool) -> McpToolSchema {
        let mut description = tool.description.clone();

        // Add MAX constraints to description if present
        let mut constraints = Vec::new();
        if tool.timeout_max != 300 {
            constraints.push(format!("Maximum timeout: {} seconds", tool.timeout_max));
        }
        if tool.stop_after_max != 3600 {
            constraints.push(format!(
                "Maximum stop_after: {} seconds",
                tool.stop_after_max
            ));
        }
        if tool.output_head_lines_max != 1000 {
            constraints.push(format!(
                "Maximum output_head_lines: {}",
                tool.output_head_lines_max
            ));
        }
        if tool.output_tail_lines_max != 1000 {
            constraints.push(format!(
                "Maximum output_tail_lines: {}",
                tool.output_tail_lines_max
            ));
        }
        if tool.stderr_lines_max != 500 {
            constraints.push(format!("Maximum stderr_lines: {}", tool.stderr_lines_max));
        }

        if !constraints.is_empty() {
            description.push_str("\n\nConstraints: ");
            description.push_str(&constraints.join(", "));
        }

        // Build parameter schema
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for (param_name, param) in &tool.parameters {
            let mut param_schema = serde_json::Map::new();
            param_schema.insert(
                "type".to_string(),
                serde_json::Value::String("string".to_string()),
            );
            param_schema.insert(
                "description".to_string(),
                serde_json::Value::String(param.description.clone()),
            );

            if let Some(example) = &param.example {
                param_schema.insert(
                    "example".to_string(),
                    serde_json::Value::String(example.clone()),
                );
            }

            properties.insert(param_name.clone(), serde_json::Value::Object(param_schema));

            if param.required {
                required.push(param_name.clone());
            }
        }

        // Add runtime override parameters
        let mut timeout_schema = serde_json::Map::new();
        timeout_schema.insert(
            "type".to_string(),
            serde_json::Value::String("number".to_string()),
        );
        timeout_schema.insert(
            "description".to_string(),
            serde_json::Value::String(format!(
                "Timeout in seconds (default: {}, max: {})",
                tool.timeout, tool.timeout_max
            )),
        );
        properties.insert(
            "timeout".to_string(),
            serde_json::Value::Object(timeout_schema),
        );

        let mut stop_after_schema = serde_json::Map::new();
        stop_after_schema.insert(
            "type".to_string(),
            serde_json::Value::String("number".to_string()),
        );
        stop_after_schema.insert(
            "description".to_string(),
            serde_json::Value::String(format!(
                "Stop after duration in seconds for long-running processes (default: {}, max: {})",
                tool.stop_after, tool.stop_after_max
            )),
        );
        properties.insert(
            "stop_after".to_string(),
            serde_json::Value::Object(stop_after_schema),
        );

        let mut output_head_schema = serde_json::Map::new();
        output_head_schema.insert(
            "type".to_string(),
            serde_json::Value::String("number".to_string()),
        );
        output_head_schema.insert(
            "description".to_string(),
            serde_json::Value::String(format!(
                "Number of lines from head of output (default: {}, max: {})",
                tool.output_head_lines, tool.output_head_lines_max
            )),
        );
        properties.insert(
            "output_head_lines".to_string(),
            serde_json::Value::Object(output_head_schema),
        );

        let mut output_tail_schema = serde_json::Map::new();
        output_tail_schema.insert(
            "type".to_string(),
            serde_json::Value::String("number".to_string()),
        );
        output_tail_schema.insert(
            "description".to_string(),
            serde_json::Value::String(format!(
                "Number of lines from tail of output (default: {}, max: {})",
                tool.output_tail_lines, tool.output_tail_lines_max
            )),
        );
        properties.insert(
            "output_tail_lines".to_string(),
            serde_json::Value::Object(output_tail_schema),
        );

        let mut stderr_schema = serde_json::Map::new();
        stderr_schema.insert(
            "type".to_string(),
            serde_json::Value::String("number".to_string()),
        );
        stderr_schema.insert(
            "description".to_string(),
            serde_json::Value::String(format!(
                "Number of lines from stderr to return on error (default: {}, max: {})",
                tool.stderr_lines, tool.stderr_lines_max
            )),
        );
        properties.insert(
            "stderr_lines".to_string(),
            serde_json::Value::Object(stderr_schema),
        );

        McpToolSchema {
            name: tool.full_name.clone(),
            description,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required,
            }),
        }
    }

    /// Validate runtime override values against MAX constraints
    pub fn validate_runtime_overrides(
        &self,
        tool: &ResolvedTool,
        timeout: Option<u64>,
        stop_after: Option<u64>,
        output_head_lines: Option<u64>,
        output_tail_lines: Option<u64>,
        stderr_lines: Option<u64>,
    ) -> Result<()> {
        if let Some(timeout_val) = timeout
            && timeout_val > tool.timeout_max {
                return Err(crate::error::McpError::OverrideExceedsMax {
                    field: "timeout".to_string(),
                    value: timeout_val,
                    max: tool.timeout_max,
                }
                .into());
            }

        if let Some(stop_after_val) = stop_after
            && stop_after_val > tool.stop_after_max {
                return Err(crate::error::McpError::OverrideExceedsMax {
                    field: "stop_after".to_string(),
                    value: stop_after_val,
                    max: tool.stop_after_max,
                }
                .into());
            }

        if let Some(head_lines) = output_head_lines
            && head_lines > tool.output_head_lines_max {
                return Err(crate::error::McpError::OverrideExceedsMax {
                    field: "output_head_lines".to_string(),
                    value: head_lines,
                    max: tool.output_head_lines_max,
                }
                .into());
            }

        if let Some(tail_lines) = output_tail_lines
            && tail_lines > tool.output_tail_lines_max {
                return Err(crate::error::McpError::OverrideExceedsMax {
                    field: "output_tail_lines".to_string(),
                    value: tail_lines,
                    max: tool.output_tail_lines_max,
                }
                .into());
            }

        if let Some(stderr_lines_val) = stderr_lines
            && stderr_lines_val > tool.stderr_lines_max {
                return Err(crate::error::McpError::OverrideExceedsMax {
                    field: "stderr_lines".to_string(),
                    value: stderr_lines_val,
                    max: tool.stderr_lines_max,
                }
                .into());
            }

        Ok(())
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
default_output_head_lines = 100
default_output_head_lines_max = 1000

  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
  
    [groups.test_group.tools.parameters.arg1]
    description = "First argument"
    required = true
    
    [groups.test_group.tools.parameters.arg2]
    description = "Second argument"
    example = "example"
    required = false
"#;
        Config::from_str(toml).unwrap()
    }

    #[test]
    fn test_tool_registry_creation() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        assert_eq!(registry.tools.len(), 1);
        assert!(registry.get_tool("test_group_test_tool").is_some());
    }

    #[test]
    fn test_tool_name_generation() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();
        assert_eq!(tool.full_name, "test_group_test_tool");
        assert_eq!(tool.group_name, "test_group");
        assert_eq!(tool.tool_name, "test_tool");
    }

    #[test]
    fn test_tool_not_found() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        assert!(registry.get_tool("nonexistent_tool").is_none());
    }

    #[test]
    fn test_generate_tool_schema() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();
        let schema = registry.generate_tool_schema(tool);

        assert_eq!(schema.name, "test_group_test_tool");
        assert!(schema.description.contains("A test tool"));
        assert!(schema.input_schema.get("type").unwrap().as_str().unwrap() == "object");
    }

    #[test]
    fn test_schema_includes_parameters() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();
        let schema = registry.generate_tool_schema(tool);

        let properties = schema
            .input_schema
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(properties.contains_key("arg1"));
        assert!(properties.contains_key("arg2"));

        let required = schema
            .input_schema
            .get("required")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(required.iter().any(|v| v.as_str().unwrap() == "arg1"));
        assert!(!required.iter().any(|v| v.as_str().unwrap() == "arg2"));
    }

    #[test]
    fn test_schema_includes_runtime_overrides() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();
        let schema = registry.generate_tool_schema(tool);

        let properties = schema
            .input_schema
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(properties.contains_key("timeout"));
        assert!(properties.contains_key("stop_after"));
        assert!(properties.contains_key("output_head_lines"));
        assert!(properties.contains_key("output_tail_lines"));
        assert!(properties.contains_key("stderr_lines"));
    }

    #[test]
    fn test_validate_runtime_overrides_within_max() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();

        // Valid overrides (within MAX)
        assert!(registry
            .validate_runtime_overrides(
                tool,
                Some(100), // timeout < 300 (max)
                None,
                Some(500), // output_head_lines < 1000 (max)
                None,
                None,
            )
            .is_ok());
    }

    #[test]
    fn test_validate_runtime_overrides_exceeds_max() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();

        // Invalid override (exceeds MAX)
        let result = registry.validate_runtime_overrides(
            tool,
            Some(500), // timeout > 300 (max)
            None,
            None,
            None,
            None,
        );
        assert!(result.is_err());

        if let Err(e) = result {
            if let crate::error::GenMcpError::Mcp(crate::error::McpError::OverrideExceedsMax {
                field,
                value,
                max,
            }) = e
            {
                assert_eq!(field, "timeout");
                assert_eq!(value, 500);
                assert_eq!(max, 300);
            } else {
                panic!("Unexpected error type");
            }
        }
    }

    #[test]
    fn test_validate_runtime_overrides_none() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();

        // No overrides (all None)
        assert!(registry
            .validate_runtime_overrides(tool, None, None, None, None, None,)
            .is_ok());
    }

    #[test]
    fn test_all_tools_iterator() {
        let toml = r#"
[groups.group1]
  [[groups.group1.tools]]
  name = "tool1"
  description = "Tool 1"
  command = "/bin/echo"

[groups.group2]
  [[groups.group2.tools]]
  name = "tool2"
  description = "Tool 2"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        let registry = ToolRegistry::from_config(&config).unwrap();

        let tools: Vec<_> = registry.all_tools().collect();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_duplicate_tool_error() {
        let toml = r#"
[groups.group1]
  [[groups.group1.tools]]
  name = "tool"
  description = "Tool"
  command = "/bin/echo"

[groups.group2]
  [[groups.group2.tools]]
  name = "tool"
  description = "Tool"
  command = "/bin/echo"
"#;
        // This should work - different groups, same tool name
        let config = Config::from_str(toml).unwrap();
        let registry = ToolRegistry::from_config(&config).unwrap();
        assert_eq!(registry.tools.len(), 2);
        assert!(registry.get_tool("group1_tool").is_some());
        assert!(registry.get_tool("group2_tool").is_some());
    }
}

/// MCP tool schema for tool registration
#[derive(Debug, Clone)]
pub struct McpToolSchema {
    /// Tool name
    pub name: String,
    /// Tool description (includes MAX constraints)
    pub description: String,
    /// Input schema (JSON Schema)
    pub input_schema: serde_json::Value,
}
