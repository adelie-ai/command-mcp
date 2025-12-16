// Integration tests for MCP handshake and initialization

use genmcp::config::Config;
use genmcp::server::McpServer;

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
    
    let config = Config::from_str(toml).unwrap();
    let server = McpServer::new(config).unwrap();
    
    // Initialize
    let capabilities = server.handle_initialize(
        "2024-11-05",
        &serde_json::json!({}),
    ).await.unwrap();
    
    assert!(capabilities.get("protocolVersion").is_some());
    assert_eq!(capabilities.get("protocolVersion").unwrap().as_str().unwrap(), "2024-11-05");
    
    // Send initialized notification
    server.handle_initialized().await.unwrap();
    assert!(server.is_initialized().await);
    
    // Shutdown
    server.handle_shutdown().await.unwrap();
    assert!(!server.is_initialized().await);
}

