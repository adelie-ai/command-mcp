#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// TOML configuration parsing and validation

use crate::error::{ConfigError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Termination signal type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TerminationSignal {
    #[serde(rename = "SIGTERM")]
    Sigterm,
    #[serde(rename = "SIGINT")]
    Sigint,
}

impl TerminationSignal {
    /// Get signal name as string
    pub fn as_str(self) -> &'static str {
        match self {
            TerminationSignal::Sigterm => "SIGTERM",
            TerminationSignal::Sigint => "SIGINT",
        }
    }
}

impl std::str::FromStr for TerminationSignal {
    type Err = ConfigError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "SIGTERM" => Ok(TerminationSignal::Sigterm),
            "SIGINT" => Ok(TerminationSignal::Sigint),
            _ => Err(ConfigError::InvalidSignal(s.to_string())),
        }
    }
}

/// Parameter definition for a tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Parameter {
    /// Description of the parameter
    pub description: String,
    /// Example value
    #[serde(default)]
    pub example: Option<String>,
    /// Whether the parameter is required
    #[serde(default = "default_false")]
    pub required: bool,
}

fn default_false() -> bool {
    false
}

/// Tool configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tool {
    /// Base name of the tool (will be prefixed with group name)
    pub name: String,
    /// Description for the LLM
    pub description: String,
    /// Command to execute
    pub command: String,
    /// Timeout in seconds (optional, overrides group default)
    #[serde(default)]
    pub timeout: Option<u64>,
    /// Maximum timeout in seconds (optional, overrides group default)
    #[serde(default)]
    pub timeout_max: Option<u64>,
    /// Stop after duration in seconds (optional, overrides group default)
    #[serde(default)]
    pub stop_after: Option<u64>,
    /// Maximum stop_after in seconds (optional, overrides group default)
    #[serde(default)]
    pub stop_after_max: Option<u64>,
    /// Termination signal (optional, overrides group default)
    #[serde(default)]
    pub termination_signal: Option<String>,
    /// Termination grace period in seconds (optional, overrides group default)
    #[serde(default)]
    pub termination_grace_period: Option<u64>,
    /// Output head lines (optional, overrides group default)
    #[serde(default)]
    pub output_head_lines: Option<u64>,
    /// Output tail lines (optional, overrides group default)
    #[serde(default)]
    pub output_tail_lines: Option<u64>,
    /// Maximum output head lines (optional, overrides group default)
    #[serde(default)]
    pub output_head_lines_max: Option<u64>,
    /// Maximum output tail lines (optional, overrides group default)
    #[serde(default)]
    pub output_tail_lines_max: Option<u64>,
    /// STDERR lines to capture (optional, overrides group default)
    #[serde(default)]
    pub stderr_lines: Option<u64>,
    /// Maximum STDERR lines (optional, overrides group default)
    #[serde(default)]
    pub stderr_lines_max: Option<u64>,
    /// Tool parameters
    #[serde(default)]
    pub parameters: HashMap<String, Parameter>,
}

/// Group configuration with defaults
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Group {
    /// Default timeout in seconds
    #[serde(default)]
    pub default_timeout: Option<u64>,
    /// Maximum timeout in seconds
    #[serde(default)]
    pub default_timeout_max: Option<u64>,
    /// Default stop_after in seconds (0 = disabled)
    #[serde(default)]
    pub default_stop_after: Option<u64>,
    /// Maximum stop_after in seconds
    #[serde(default)]
    pub default_stop_after_max: Option<u64>,
    /// Default termination signal
    #[serde(default)]
    pub default_termination_signal: Option<String>,
    /// Default termination grace period in seconds
    #[serde(default)]
    pub default_termination_grace_period: Option<u64>,
    /// Default output head lines
    #[serde(default)]
    pub default_output_head_lines: Option<u64>,
    /// Default output tail lines
    #[serde(default)]
    pub default_output_tail_lines: Option<u64>,
    /// Maximum output head lines
    #[serde(default)]
    pub default_output_head_lines_max: Option<u64>,
    /// Maximum output tail lines
    #[serde(default)]
    pub default_output_tail_lines_max: Option<u64>,
    /// Default STDERR lines
    #[serde(default)]
    pub default_stderr_lines: Option<u64>,
    /// Maximum STDERR lines
    #[serde(default)]
    pub default_stderr_lines_max: Option<u64>,
    /// Tools in this group
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// Root configuration structure matching TOML format
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigToml {
    /// Groups of tools
    #[serde(default)]
    pub groups: HashMap<String, Group>,
}

/// Root configuration structure
#[derive(Debug, Clone)]
pub struct Config {
    /// Groups of tools
    pub groups: HashMap<String, Group>,
}

/// Resolved tool configuration with all defaults applied
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    /// Full tool name: {group_name}_{tool_name}
    pub full_name: String,
    /// Group name
    pub group_name: String,
    /// Base tool name
    pub tool_name: String,
    /// Description
    pub description: String,
    /// Command to execute
    pub command: String,
    /// Timeout in seconds
    pub timeout: u64,
    /// Maximum timeout in seconds
    pub timeout_max: u64,
    /// Stop after duration in seconds (0 = disabled)
    pub stop_after: u64,
    /// Maximum stop_after in seconds
    pub stop_after_max: u64,
    /// Termination signal
    pub termination_signal: TerminationSignal,
    /// Termination grace period in seconds
    pub termination_grace_period: u64,
    /// Output head lines
    pub output_head_lines: u64,
    /// Output tail lines
    pub output_tail_lines: u64,
    /// Maximum output head lines
    pub output_head_lines_max: u64,
    /// Maximum output tail lines
    pub output_tail_lines_max: u64,
    /// STDERR lines to capture
    pub stderr_lines: u64,
    /// Maximum STDERR lines
    pub stderr_lines_max: u64,
    /// Tool parameters
    pub parameters: HashMap<String, Parameter>,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ConfigError::FileNotFound(format!("{}: {}", path.as_ref().display(), e)))?;
        content.parse()
    }

    /// Parse configuration from TOML string
    #[allow(clippy::should_implement_trait)] // We implement FromStr trait instead
    pub fn from_str(content: &str) -> Result<Self> {
        content.parse()
    }

    /// Validate configuration
    fn validate(&mut self) -> Result<()> {
        let mut tool_names = std::collections::HashSet::new();

        for (group_name, group) in &mut self.groups {
            // Validate group defaults
            Self::validate_timeout_pair(
                group.default_timeout,
                group.default_timeout_max,
                &format!("groups.{}.default_timeout", group_name),
            )?;

            Self::validate_timeout_pair(
                group.default_stop_after,
                group.default_stop_after_max,
                &format!("groups.{}.default_stop_after", group_name),
            )?;

            Self::validate_output_pair(
                group.default_output_head_lines,
                group.default_output_head_lines_max,
                &format!("groups.{}.default_output_head_lines", group_name),
            )?;

            Self::validate_output_pair(
                group.default_output_tail_lines,
                group.default_output_tail_lines_max,
                &format!("groups.{}.default_output_tail_lines", group_name),
            )?;

            Self::validate_output_pair(
                group.default_stderr_lines,
                group.default_stderr_lines_max,
                &format!("groups.{}.default_stderr_lines", group_name),
            )?;

            // Validate termination signal
            if let Some(ref signal_str) = group.default_termination_signal {
                signal_str.parse::<TerminationSignal>()
                    .map_err(|_| ConfigError::InvalidSignal(signal_str.clone()))?;
            }

            // Validate tools
            for tool in &group.tools {
                let full_name = format!("{}_{}", group_name, tool.name);

                if tool_names.contains(&full_name) {
                    return Err(ConfigError::DuplicateToolName(full_name).into());
                }
                tool_names.insert(full_name.clone());

                // Validate tool overrides
                if let Some(timeout) = tool.timeout {
                    if timeout == 0 {
                        return Err(ConfigError::InvalidTimeout(0).into());
                    }
                }

                if let Some(ref signal_str) = tool.termination_signal {
                    signal_str.parse::<TerminationSignal>()
                        .map_err(|_| ConfigError::InvalidSignal(signal_str.clone()))?;
                }

                // Validate tool timeout pair
                Self::validate_timeout_pair(
                    tool.timeout,
                    tool.timeout_max,
                    &format!("groups.{}.tools[{}].timeout", group_name, tool.name),
                )?;

                Self::validate_timeout_pair(
                    tool.stop_after,
                    tool.stop_after_max,
                    &format!("groups.{}.tools[{}].stop_after", group_name, tool.name),
                )?;

                Self::validate_output_pair(
                    tool.output_head_lines,
                    tool.output_head_lines_max,
                    &format!("groups.{}.tools[{}].output_head_lines", group_name, tool.name),
                )?;

                Self::validate_output_pair(
                    tool.output_tail_lines,
                    tool.output_tail_lines_max,
                    &format!("groups.{}.tools[{}].output_tail_lines", group_name, tool.name),
                )?;

                Self::validate_output_pair(
                    tool.stderr_lines,
                    tool.stderr_lines_max,
                    &format!("groups.{}.tools[{}].stderr_lines", group_name, tool.name),
                )?;
            }
        }

        Ok(())
    }

    /// Validate that MAX >= default for timeout/stop_after values
    fn validate_timeout_pair(
        default: Option<u64>,
        max: Option<u64>,
        field_name: &str,
    ) -> Result<()> {
        if let (Some(default_val), Some(max_val)) = (default, max) {
            if max_val < default_val {
                return Err(ConfigError::InvalidMax {
                    field: field_name.to_string(),
                    default: default_val,
                    max: max_val,
                }.into());
            }
        }
        Ok(())
    }

    /// Validate that MAX >= default for output values
    fn validate_output_pair(
        default: Option<u64>,
        max: Option<u64>,
        field_name: &str,
    ) -> Result<()> {
        if let (Some(default_val), Some(max_val)) = (default, max) {
            if max_val < default_val {
                return Err(ConfigError::InvalidMax {
                    field: field_name.to_string(),
                    default: default_val,
                    max: max_val,
                }.into());
            }
        }
        Ok(())
    }

    /// Resolve a tool configuration with all defaults applied
    pub fn resolve_tool(&self, group_name: &str, tool: &Tool) -> Result<ResolvedTool> {
        let group = self.groups.get(group_name)
            .ok_or_else(|| ConfigError::InvalidValue {
                field: "group_name".to_string(),
                message: format!("Group '{}' not found", group_name),
            })?;

        let full_name = format!("{}_{}", group_name, tool.name);

        // Resolve termination signal
        let default_signal = "SIGTERM".to_string();
        let termination_signal_str = tool.termination_signal
            .as_ref()
            .or(group.default_termination_signal.as_ref())
            .unwrap_or(&default_signal);
        let termination_signal = termination_signal_str.parse::<TerminationSignal>()
            .map_err(|_| ConfigError::InvalidSignal(termination_signal_str.clone()))?;

        Ok(ResolvedTool {
            full_name,
            group_name: group_name.to_string(),
            tool_name: tool.name.clone(),
            description: tool.description.clone(),
            command: tool.command.clone(),
            timeout: tool.timeout.unwrap_or(group.default_timeout.unwrap_or(30)),
            timeout_max: tool.timeout_max.or(group.default_timeout_max).unwrap_or(300),
            stop_after: tool.stop_after.unwrap_or(group.default_stop_after.unwrap_or(0)),
            stop_after_max: tool.stop_after_max.or(group.default_stop_after_max).unwrap_or(3600),
            termination_signal,
            termination_grace_period: tool.termination_grace_period
                .unwrap_or(group.default_termination_grace_period.unwrap_or(5)),
            output_head_lines: tool.output_head_lines
                .unwrap_or(group.default_output_head_lines.unwrap_or(100)),
            output_tail_lines: tool.output_tail_lines
                .unwrap_or(group.default_output_tail_lines.unwrap_or(100)),
            output_head_lines_max: tool.output_head_lines_max
                .or(group.default_output_head_lines_max)
                .unwrap_or(1000),
            output_tail_lines_max: tool.output_tail_lines_max
                .or(group.default_output_tail_lines_max)
                .unwrap_or(1000),
            stderr_lines: tool.stderr_lines
                .unwrap_or(group.default_stderr_lines.unwrap_or(50)),
            stderr_lines_max: tool.stderr_lines_max
                .or(group.default_stderr_lines_max)
                .unwrap_or(500),
            parameters: tool.parameters.clone(),
        })
    }

    /// Get all resolved tools
    pub fn get_all_tools(&self) -> Result<Vec<ResolvedTool>> {
        let mut tools = Vec::new();

        for (group_name, group) in &self.groups {
            for tool in &group.tools {
                tools.push(self.resolve_tool(group_name, tool)?);
            }
        }

        Ok(tools)
    }
}

impl std::str::FromStr for Config {
    type Err = crate::error::GenMcpError;

    /// Parse configuration from TOML string
    fn from_str(content: &str) -> std::result::Result<Self, Self::Err> {
        let config_toml: ConfigToml = toml::from_str(content)
            .map_err(ConfigError::ParseToml)?;

        let mut config = Config {
            groups: config_toml.groups,
        };

        // Validate configuration
        config.validate()?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config_parsing() {
        let toml = r#"
[groups.test_group]
default_timeout = 30
default_timeout_max = 300

  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.groups.len(), 1);
        assert!(config.groups.contains_key("test_group"));
    }

    #[test]
    fn test_missing_optional_fields() {
        let toml = r#"
[groups.test_group]
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        assert_eq!(group.tools.len(), 1);
        // Should use defaults
        let tool = &group.tools[0];
        assert_eq!(tool.name, "test_tool");
    }

    #[test]
    fn test_invalid_toml() {
        let toml = "invalid toml content {";
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_duplicate_tool_names() {
        let toml = r#"
[groups.test_group]
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "Another test tool"
  command = "/bin/echo"
"#;
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_invalid_max_value() {
        let toml = r#"
[groups.test_group]
default_timeout = 300
default_timeout_max = 30
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_invalid_signal() {
        let toml = r#"
[groups.test_group]
default_termination_signal = "INVALID"
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_resolve_tool_with_defaults() {
        let toml = r#"
[groups.test_group]
default_timeout = 60
default_output_head_lines = 200
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        let tool = &group.tools[0];
        let resolved = config.resolve_tool("test_group", tool).unwrap();
        assert_eq!(resolved.full_name, "test_group_test_tool");
        assert_eq!(resolved.timeout, 60);
        assert_eq!(resolved.output_head_lines, 200);
    }

    #[test]
    fn test_resolve_tool_with_overrides() {
        let toml = r#"
[groups.test_group]
default_timeout = 60
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
  timeout = 120
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        let tool = &group.tools[0];
        let resolved = config.resolve_tool("test_group", tool).unwrap();
        assert_eq!(resolved.timeout, 120);
    }

    #[test]
    fn test_empty_groups() {
        let toml = r#"
[groups.empty_group]
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.groups.len(), 1);
        let group = config.groups.get("empty_group").unwrap();
        assert_eq!(group.tools.len(), 0);
    }

    #[test]
    fn test_multiple_groups() {
        let toml = r#"
[groups.group1]
default_timeout = 30

  [[groups.group1.tools]]
  name = "tool1"
  description = "Tool 1"
  command = "/bin/echo"

[groups.group2]
default_timeout = 60

  [[groups.group2.tools]]
  name = "tool2"
  description = "Tool 2"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.groups.len(), 2);
        let tools = config.get_all_tools().unwrap();
        assert_eq!(tools.len(), 2);
        
        // Order is not guaranteed, so check that both tools exist
        let tool_names: Vec<String> = tools.iter().map(|t| t.full_name.clone()).collect();
        assert!(tool_names.contains(&"group1_tool1".to_string()));
        assert!(tool_names.contains(&"group2_tool2".to_string()));
    }

    #[test]
    fn test_parameter_definitions() {
        let toml = r#"
[groups.test_group]
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
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        let tool = &group.tools[0];
        let resolved = config.resolve_tool("test_group", tool).unwrap();
        assert_eq!(resolved.parameters.len(), 2);
        assert!(resolved.parameters.contains_key("arg1"));
        assert!(resolved.parameters.contains_key("arg2"));
        assert!(resolved.parameters.get("arg1").unwrap().required);
        assert!(!resolved.parameters.get("arg2").unwrap().required);
    }

    #[test]
    fn test_termination_signal_parsing() {
        let toml = r#"
[groups.test_group]
default_termination_signal = "SIGINT"
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
  termination_signal = "SIGTERM"
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        let tool = &group.tools[0];
        let resolved = config.resolve_tool("test_group", tool).unwrap();
        assert_eq!(resolved.termination_signal, crate::config::TerminationSignal::Sigterm);
    }

    #[test]
    fn test_default_inheritance_chain() {
        let toml = r#"
[groups.test_group]
default_timeout = 30
default_output_head_lines = 100
  [[groups.test_group.tools]]
  name = "tool1"
  description = "Tool 1"
  command = "/bin/echo"
  # Uses all group defaults
  
  [[groups.test_group.tools]]
  name = "tool2"
  description = "Tool 2"
  command = "/bin/echo"
  timeout = 60
  # Overrides timeout, inherits output_head_lines
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        
        let tool1 = &group.tools[0];
        let resolved1 = config.resolve_tool("test_group", tool1).unwrap();
        assert_eq!(resolved1.timeout, 30);
        assert_eq!(resolved1.output_head_lines, 100);
        
        let tool2 = &group.tools[1];
        let resolved2 = config.resolve_tool("test_group", tool2).unwrap();
        assert_eq!(resolved2.timeout, 60);
        assert_eq!(resolved2.output_head_lines, 100);
    }

    #[test]
    fn test_stop_after_zero_disabled() {
        let toml = r#"
[groups.test_group]
default_stop_after = 0
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        let config = Config::from_str(toml).unwrap();
        let group = config.groups.get("test_group").unwrap();
        let tool = &group.tools[0];
        let resolved = config.resolve_tool("test_group", tool).unwrap();
        assert_eq!(resolved.stop_after, 0);
    }

    #[test]
    fn test_max_value_validation() {
        let toml = r#"
[groups.test_group]
default_timeout = 300
default_timeout_max = 30
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_invalid_field_types() {
        let toml = r#"
[groups.test_group]
default_timeout = "not_a_number"
  [[groups.test_group.tools]]
  name = "test_tool"
  description = "A test tool"
  command = "/bin/echo"
"#;
        assert!(Config::from_str(toml).is_err());
    }

    #[test]
    fn test_missing_required_fields() {
        let toml = r#"
[groups.test_group]
  [[groups.test_group.tools]]
  name = "test_tool"
  # Missing description and command
"#;
        assert!(Config::from_str(toml).is_err());
    }
}
