// Integration tests for the MCP handshake, driven through mcp-core's Session
// (which now owns the JSON-RPC protocol) wrapping command-mcp's dynamic service.

use command_mcp::config::Config;
use command_mcp::service::CommandMcpService;
use mcp_core::{ServerConfig, ServerCore, Session};
use serde_json::json;
use std::sync::Arc;

fn session_for(toml: &str) -> Session {
    let config = Config::from_str(toml).unwrap();
    let service = CommandMcpService::new(config).unwrap();
    let core = ServerCore::new(
        ServerConfig::new("command-mcp", env!("CARGO_PKG_VERSION")),
        Arc::new(service),
    );
    Session::new(core)
}

#[tokio::test]
async fn test_full_initialization_flow() {
    let toml = r#"
[groups.test_group]
default_timeout = 30

  [[groups.test_group.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
"#;

    let mut session = session_for(toml);

    // Initialize — mcp-core negotiates the requested version and does NOT leak a
    // top-level `tools` key (that was a command-mcp quirk; the spec-correct shape
    // returns tools only via tools/list).
    let init = session
        .handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
        }))
        .await;
    let result = &init.response.unwrap()["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "command-mcp");
    assert!(
        result.get("tools").is_none(),
        "initialize must not embed a top-level tools key"
    );

    // tools/list returns the dynamically generated tool.
    let list = session
        .handle_message(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let tools = list.response.unwrap()["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "test_group_echo");

    // The `initialized` notification gets no response.
    let notif = session
        .handle_message(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    assert!(notif.response.is_none());
}
