#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Tool registry and MCP tool definitions

use crate::config::{Config, ParamType, ResolvedTool};
use crate::error::{Result, ToolRegistryError};
use std::collections::HashMap;

/// Tool registry that manages all available tools
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    /// Map of full tool name to resolved tool configuration
    tools: HashMap<String, ResolvedTool>,
    /// Whether the 5 runtime-override knobs are advertised in the generated
    /// input schema. They are always *honored* at call time regardless.
    expose_runtime_overrides: bool,
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

        Ok(ToolRegistry {
            tools,
            expose_runtime_overrides: config.expose_runtime_overrides,
        })
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

            // Resolve the JSON type. An explicit `type` wins; otherwise infer
            // from the CLI emission knobs (back-compat): a flag-only parameter
            // (a flag with no value) is a boolean toggle; everything else
            // (positional values and flag-with-value) is a string.
            let json_type = match param.param_type {
                ParamType::Infer => {
                    if param.flag.is_some() && !param.takes_value {
                        "boolean"
                    } else {
                        "string"
                    }
                }
                ParamType::String => "string",
                ParamType::Integer => "integer",
                ParamType::Number => "number",
                ParamType::Boolean => "boolean",
                // Enum is a constrained string.
                ParamType::Enum => "string",
            };
            param_schema.insert(
                "type".to_string(),
                serde_json::Value::String(json_type.to_string()),
            );
            param_schema.insert(
                "description".to_string(),
                serde_json::Value::String(param.description.clone()),
            );

            // Emit an `enum` constraint when allowed values are configured (for
            // an explicit `type = "enum"` or simply whenever `enum` is set).
            if let Some(values) = &param.r#enum {
                param_schema.insert(
                    "enum".to_string(),
                    serde_json::Value::Array(
                        values
                            .iter()
                            .map(|v| serde_json::Value::String(v.clone()))
                            .collect(),
                    ),
                );
            }

            if let Some(default) = &param.default {
                param_schema.insert("default".to_string(), default.clone());
            }
            if let Some(minimum) = param.minimum
                && let Some(n) = serde_json::Number::from_f64(minimum)
            {
                param_schema.insert("minimum".to_string(), serde_json::Value::Number(n));
            }
            if let Some(maximum) = param.maximum
                && let Some(n) = serde_json::Number::from_f64(maximum)
            {
                param_schema.insert("maximum".to_string(), serde_json::Value::Number(n));
            }

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

        // The 5 runtime-override knobs are only *advertised* when the config
        // opts in via `expose_runtime_overrides`. They are still honored at call
        // time regardless (see `service::call_tool`).
        if self.expose_runtime_overrides {
            Self::add_runtime_override_properties(tool, &mut properties);
        }

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

    /// Add the 5 runtime-override knobs to a tool's input-schema properties.
    fn add_runtime_override_properties(
        tool: &ResolvedTool,
        properties: &mut serde_json::Map<String, serde_json::Value>,
    ) {
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
            && timeout_val > tool.timeout_max
        {
            return Err(crate::error::McpError::OverrideExceedsMax {
                field: "timeout".to_string(),
                value: timeout_val,
                max: tool.timeout_max,
            }
            .into());
        }

        if let Some(stop_after_val) = stop_after
            && stop_after_val > tool.stop_after_max
        {
            return Err(crate::error::McpError::OverrideExceedsMax {
                field: "stop_after".to_string(),
                value: stop_after_val,
                max: tool.stop_after_max,
            }
            .into());
        }

        if let Some(head_lines) = output_head_lines
            && head_lines > tool.output_head_lines_max
        {
            return Err(crate::error::McpError::OverrideExceedsMax {
                field: "output_head_lines".to_string(),
                value: head_lines,
                max: tool.output_head_lines_max,
            }
            .into());
        }

        if let Some(tail_lines) = output_tail_lines
            && tail_lines > tool.output_tail_lines_max
        {
            return Err(crate::error::McpError::OverrideExceedsMax {
                field: "output_tail_lines".to_string(),
                value: tail_lines,
                max: tool.output_tail_lines_max,
            }
            .into());
        }

        if let Some(stderr_lines_val) = stderr_lines
            && stderr_lines_val > tool.stderr_lines_max
        {
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
    fn test_schema_param_types_reflect_flags() {
        // A flag-only parameter is a boolean toggle; a flag-with-value and a
        // plain positional parameter are strings.
        let toml = r#"
[groups.g]
  [[groups.g.tools]]
  name = "tool"
  description = "tool"
  command = "/bin/echo"

    [groups.g.tools.parameters.verbose]
    description = "verbose flag"
    flag = "-v"

    [groups.g.tools.parameters.count]
    description = "count"
    flag = "-n"
    takes_value = true

    [groups.g.tools.parameters.path]
    description = "path"
"#;
        let config = Config::from_str(toml).unwrap();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("g_tool").unwrap();
        let schema = registry.generate_tool_schema(tool);

        let properties = schema
            .input_schema
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();

        let ty = |name: &str| {
            properties
                .get(name)
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str())
                .unwrap()
        };
        assert_eq!(ty("verbose"), "boolean");
        assert_eq!(ty("count"), "string");
        assert_eq!(ty("path"), "string");
    }

    #[test]
    fn test_runtime_overrides_absent_by_default() {
        // By default the runtime-override knobs are NOT advertised in the schema.
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
        assert!(!properties.contains_key("timeout"));
        assert!(!properties.contains_key("stop_after"));
        assert!(!properties.contains_key("output_head_lines"));
        assert!(!properties.contains_key("output_tail_lines"));
        assert!(!properties.contains_key("stderr_lines"));
    }

    #[test]
    fn test_schema_includes_runtime_overrides_when_exposed() {
        // With `expose_runtime_overrides = true` the 5 knobs are advertised.
        let toml = r#"
expose_runtime_overrides = true

[groups.test_group]
default_timeout = 30
default_timeout_max = 300

  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"

    [groups.test_group.tools.parameters.arg1]
    description = "First argument"
    required = true
"#;
        let config = Config::from_str(toml).unwrap();
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
    fn test_typed_param_schema_emits_constraints() {
        // Explicit type/enum/default/min/max are emitted precisely.
        let toml = r#"
[groups.g]
  [[groups.g.tools]]
  name = "tool"
  description = "tool"
  command = "/bin/echo"

    [groups.g.tools.parameters.count]
    description = "count"
    type = "integer"
    default = 5
    minimum = 1
    maximum = 100

    [groups.g.tools.parameters.mode]
    description = "mode"
    type = "enum"
    enum = ["fast", "slow"]
"#;
        let config = Config::from_str(toml).unwrap();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("g_tool").unwrap();
        let schema = registry.generate_tool_schema(tool);
        let props = schema
            .input_schema
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();

        let count = props.get("count").unwrap();
        assert_eq!(count.get("type").unwrap(), "integer");
        assert_eq!(count.get("default").unwrap(), 5);
        assert_eq!(count.get("minimum").unwrap(), 1.0);
        assert_eq!(count.get("maximum").unwrap(), 100.0);

        let mode = props.get("mode").unwrap();
        assert_eq!(mode.get("type").unwrap(), "string");
        let enum_vals = mode.get("enum").unwrap().as_array().unwrap();
        assert_eq!(enum_vals.len(), 2);
        assert_eq!(enum_vals[0], "fast");
    }

    #[test]
    fn test_validate_runtime_overrides_within_max() {
        let config = create_test_config();
        let registry = ToolRegistry::from_config(&config).unwrap();
        let tool = registry.get_tool("test_group_test_tool").unwrap();

        // Valid overrides (within MAX)
        assert!(
            registry
                .validate_runtime_overrides(
                    tool,
                    Some(100), // timeout < 300 (max)
                    None,
                    Some(500), // output_head_lines < 1000 (max)
                    None,
                    None,
                )
                .is_ok()
        );
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
        assert!(
            registry
                .validate_runtime_overrides(tool, None, None, None, None, None,)
                .is_ok()
        );
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
