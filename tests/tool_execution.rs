// Integration tests for end-to-end tool execution through the gen-mcp service.

use genmcp::config::Config;
use genmcp::service::GenMcpService;
use mcp_core::{CallError, Content, McpService};

fn service_for(toml: &str) -> GenMcpService {
    GenMcpService::new(Config::from_str(toml).unwrap()).unwrap()
}

fn reply_text(content: &[Content]) -> String {
    match &content[0] {
        Content::Text(t) => t.clone(),
        other => panic!("expected text content, got {other:?}"),
    }
}

#[tokio::test]
async fn test_tool_execution_through_service() {
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

    let service = service_for(toml);
    let reply = service
        .call_tool(
            "test_group_echo",
            &serde_json::json!({ "text": "integration test" }),
        )
        .await
        .unwrap();

    assert!(!reply.is_error);
    let text = reply_text(&reply.content);
    assert!(text.contains("Exit code: 0"));
    assert!(text.contains("integration test"));
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

    let service = service_for(toml);
    let reply = service
        .call_tool(
            "test_group_echo",
            &serde_json::json!({ "timeout": 60, "output_head_lines": 50 }),
        )
        .await
        .unwrap();

    assert!(!reply.is_error);
    assert!(reply_text(&reply.content).contains("Exit code: 0"));
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

    let service = service_for(toml);
    let reply = service
        .call_tool("test_group_false", &serde_json::json!({}))
        .await
        .unwrap();

    // A non-zero exit is surfaced as a tool-level error (isError content).
    assert!(reply.is_error);
}

#[tokio::test]
async fn test_unknown_tool_is_tool_error() {
    let toml = r#"
[groups.test_group]
  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
"#;

    let service = service_for(toml);
    let err = service
        .call_tool("does_not_exist", &serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Tool(_)));
}
