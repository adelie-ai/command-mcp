// Integration tests for end-to-end tool execution

use genmcp::config::Config;
use genmcp::server::McpServer;

#[tokio::test]
async fn test_tool_execution_through_server() {
    let toml = r#"
[groups.test_group]
default_timeout = 30

  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
  
    [groups.test_group.tools.parameters.text]
    description = "Text to echo"
    required = true
"#;
    
    let config = Config::from_str(toml).unwrap();
    let server = McpServer::new(config).unwrap();
    
    // Initialize
    server.handle_initialize(
        "2024-11-05",
        &serde_json::json!({}),
    ).await.unwrap();
    server.handle_initialized().await.unwrap();
    
    // Execute tool
    let result = server.handle_tool_call(
        "test_group_echo",
        &serde_json::json!({
            "text": "integration test"
        }),
    ).await.unwrap();
    
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("integration test"));
}

#[tokio::test]
async fn test_tool_execution_with_runtime_overrides() {
    let toml = r#"
[groups.test_group]
default_timeout = 30
default_timeout_max = 300
default_output_head_lines = 100
default_output_head_lines_max = 1000

  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
"#;
    
    let config = Config::from_str(toml).unwrap();
    let server = McpServer::new(config).unwrap();
    
    server.handle_initialize(
        "2024-11-05",
        &serde_json::json!({}),
    ).await.unwrap();
    server.handle_initialized().await.unwrap();
    
    // Execute with runtime overrides
    let result = server.handle_tool_call(
        "test_group_echo",
        &serde_json::json!({
            "timeout": 60,
            "output_head_lines": 50,
        }),
    ).await.unwrap();
    
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_tool_execution_error_propagation() {
    let toml = r#"
[groups.test_group]
default_timeout = 30

  [[groups.test_group.tools]]
  name = "false"
  description = "Always fails"
  command = "/bin/false"
"#;
    
    let config = Config::from_str(toml).unwrap();
    let server = McpServer::new(config).unwrap();
    
    server.handle_initialize(
        "2024-11-05",
        &serde_json::json!({}),
    ).await.unwrap();
    server.handle_initialized().await.unwrap();
    
    let result = server.handle_tool_call(
        "test_group_false",
        &serde_json::json!({}),
    ).await.unwrap();
    
    // Command should fail (exit code != 0)
    assert_ne!(result.exit_code, 0);
}

