#![deny(warnings)]

// Binary crate for genmcp - uses library crate

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use genmcp::config::Config;
use genmcp::error::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "genmcp")]
#[command(about = "Generic MCP Script Adapter Server")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the MCP server
    Serve {
        /// Path to TOML configuration file
        #[arg(short, long)]
        config: String,
        /// Transport mode (stdio or websocket)
        #[arg(short, long, default_value = "stdio")]
        mode: String,
        /// Port for WebSocket mode
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        /// Host for WebSocket mode
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
        /// JWT secret for WebSocket authentication (legacy, optional)
        #[arg(long)]
        jwt_secret: Option<String>,
        /// OIDC issuer URL for JWT validation via JWKS (preferred over jwt-secret)
        #[arg(long)]
        oidc_issuer: Option<String>,
    },
    /// Output configuration file schema
    Schema {
        /// Output format (json, toml, or markdown)
        #[arg(short, long, default_value = "json")]
        format: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            config,
            mode,
            port,
            host,
            jwt_secret,
            oidc_issuer,
        } => {
            // Load configuration
            let config = Config::from_file(&config)?;

            // Create server
            let server = genmcp::server::McpServer::new(config)?;

            match mode.as_str() {
                "stdio" => {
                    run_stdio_server(server).await?;
                }
                "websocket" => {
                    run_websocket_server(server, &host, port, jwt_secret, oidc_issuer).await?;
                }
                _ => {
                    eprintln!("Invalid mode: {}. Must be 'stdio' or 'websocket'", mode);
                    std::process::exit(1);
                }
            }
        }
        Commands::Schema { format } => {
            genmcp::config_schema::output_schema(&format)?;
        }
    }

    Ok(())
}

async fn run_stdio_server(server: genmcp::server::McpServer) -> Result<()> {
    use genmcp::transport::StdioTransportHandler;

    let server = Arc::new(server);
    let mut transport = StdioTransportHandler::new();

    loop {
        // Read JSON-RPC message from stdin
        let message_str = match transport.read_message().await {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Error reading message: {}", e);
                break;
            }
        };

        if message_str.is_empty() {
            continue;
        }

        // Parse JSON-RPC message
        let message: Value = match serde_json::from_str(&message_str) {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Error parsing JSON-RPC message: {}", e);
                // Send parse error response
                let error_response = jsonrpc_error_response(None, -32700, "Parse error", None);
                if let Ok(resp_str) = serde_json::to_string(&error_response) {
                    let _ = transport.write_message(&resp_str).await;
                }
                continue;
            }
        };

        // Handle message and get response
        let response = handle_jsonrpc_message(Arc::clone(&server), message).await;

        // Send response if present (notifications don't have responses)
        if let Some(resp) = response {
            let resp_str = match serde_json::to_string(&resp) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error serializing response: {}", e);
                    continue;
                }
            };
            if let Err(e) = transport.write_message(&resp_str).await {
                eprintln!("Error writing response: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn run_websocket_server(
    server: genmcp::server::McpServer,
    host: &str,
    port: u16,
    jwt_secret_override: Option<String>,
    oidc_issuer_override: Option<String>,
) -> Result<()> {
    let server = Arc::new(server);

    // Get JWT config from CLI override or server (config file)
    let jwt_config = if let Some(issuer) = oidc_issuer_override {
        Some(genmcp::server::WebSocketAuth {
            enabled: true,
            secret: None,
            oidc_issuer: Some(issuer),
            jwks_url: None,
        })
    } else if let Some(secret) = jwt_secret_override {
        Some(genmcp::server::WebSocketAuth {
            enabled: true,
            secret: Some(secret),
            oidc_issuer: None,
            jwks_url: None,
        })
    } else {
        server.websocket_auth().cloned()
    };

    // Initialize JWKS verifier if OIDC is configured
    let jwks_verifier: Option<Arc<genmcp::oidc::JwksVerifier>> = if let Some(ref auth) = jwt_config
    {
        if auth.enabled {
            if let Some(ref issuer) = auth.oidc_issuer {
                Some(Arc::new(
                    genmcp::oidc::JwksVerifier::from_oidc_issuer(issuer).await?,
                ))
            } else {
                auth.jwks_url
                    .as_ref()
                    .map(|jwks_url| Arc::new(genmcp::oidc::JwksVerifier::from_jwks_url(jwks_url)))
            }
        } else {
            None
        }
    } else {
        None
    };

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .with_state((server, jwt_config, jwks_verifier));

    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("WebSocket server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

// Type alias for WebSocket handler state
type WebSocketState = (
    Arc<genmcp::server::McpServer>,
    Option<genmcp::server::WebSocketAuth>,
    Option<Arc<genmcp::oidc::JwksVerifier>>,
);

async fn websocket_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State((server, jwt_config, jwks_verifier)): State<WebSocketState>,
) -> Response {
    // Authenticate WebSocket connection if enabled
    if let Some(ref auth) = jwt_config {
        if auth.enabled {
            if let Err(e) = validate_jwt_token(&headers, auth, jwks_verifier.as_deref()).await {
                eprintln!("WebSocket authentication failed: {}", e);
                return (
                    StatusCode::UNAUTHORIZED,
                    format!("Authentication failed: {}", e),
                )
                    .into_response();
            }
        }
        // If auth is disabled, allow connection without authentication
    } else {
        // No auth config means authentication is disabled
    }

    ws.on_upgrade(move |socket| handle_websocket_connection(socket, server))
}

async fn handle_websocket_connection(
    socket: axum::extract::ws::WebSocket,
    server: Arc<genmcp::server::McpServer>,
) {
    use axum::extract::ws::Message;

    let (mut sender, mut receiver) = socket.split();

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Parse JSON-RPC message
                let message: Value = match serde_json::from_str(&text) {
                    Ok(msg) => msg,
                    Err(e) => {
                        eprintln!("Error parsing JSON-RPC message: {}", e);
                        let error_response =
                            jsonrpc_error_response(None, -32700, "Parse error", None);
                        if let Ok(resp_str) = serde_json::to_string(&error_response) {
                            let _ = sender.send(Message::Text(resp_str.into())).await;
                        }
                        continue;
                    }
                };

                // Handle message and get response
                let response = handle_jsonrpc_message(Arc::clone(&server), message).await;

                // Send response if present
                if let Some(resp) = response {
                    if let Ok(resp_str) = serde_json::to_string(&resp) {
                        if let Err(e) = sender.send(Message::Text(resp_str.into())).await {
                            eprintln!("Error sending WebSocket response: {}", e);
                            break;
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}

async fn validate_jwt_token(
    headers: &HeaderMap,
    auth: &genmcp::server::WebSocketAuth,
    jwks_verifier: Option<&genmcp::oidc::JwksVerifier>,
) -> Result<()> {
    use genmcp::error::TransportError;

    // Extract Bearer token from header
    let auth_header = headers
        .get("authorization")
        .ok_or_else(|| TransportError::Authentication("Missing Authorization header".to_string()))?
        .to_str()
        .map_err(|_| TransportError::Authentication("Invalid Authorization header".to_string()))?;

    if !auth_header.starts_with("Bearer ") {
        return Err(TransportError::Authentication(
            "Invalid Authorization header format".to_string(),
        )
        .into());
    }

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| TransportError::Authentication("Invalid Bearer token format".to_string()))?
        .to_string();

    if token.is_empty() {
        return Err(TransportError::Authentication("Empty Bearer token".to_string()).into());
    }

    // Use JWKS verifier if available (OIDC/JWKS mode)
    if let Some(verifier) = jwks_verifier {
        let _claims = verifier.verify(&token).await?;
        // Token is valid
        return Ok(());
    }

    // Fall back to secret-based validation (legacy mode)
    if let Some(ref secret) = auth.secret {
        // Validate JWT token using secret
        let validation = jsonwebtoken::Validation::default();
        let _decoded = jsonwebtoken::decode::<serde_json::Value>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(secret.as_ref()),
            &validation,
        )
        .map_err(|e| TransportError::Authentication(format!("JWT validation failed: {}", e)))?;

        // Token is valid
        Ok(())
    } else {
        // Stub mode: just check token exists (for backward compatibility)
        Ok(())
    }
}

async fn handle_jsonrpc_message(
    server: Arc<genmcp::server::McpServer>,
    message: Value,
) -> Option<Value> {
    // Extract JSON-RPC fields
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(|m| m.as_str());
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    // Check if this is a notification (no id) or request (has id)
    let is_notification = id.is_none();

    // Handle different MCP methods
    let result = match method {
        Some("initialize") => {
            let protocol_version = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2024-11-05");
            let client_capabilities = params.get("capabilities").unwrap_or(&Value::Null);

            match server
                .handle_initialize(protocol_version, client_capabilities)
                .await
            {
                Ok(capabilities) => Ok(capabilities),
                Err(e) => Err(e),
            }
        }
        Some("initialized") | Some("notifications/initialized") => {
            match server.handle_initialized().await {
                Ok(_) => Ok(Value::Null),
                Err(e) => Err(e),
            }
        }
        Some("tools/list") => {
            // Check if server is initialized
            if !server.is_initialized().await {
                return Some(jsonrpc_error_response(
                    id,
                    -32000,
                    "Server not initialized. Call 'initialize' first.",
                    None,
                ));
            }

            Ok(serde_json::json!({ "tools": server.list_tools() }))
        }
        Some("tools/call") => {
            // Check if server is initialized
            if !server.is_initialized().await {
                return Some(jsonrpc_error_response(
                    id,
                    -32000,
                    "Server not initialized. Call 'initialize' first.",
                    None,
                ));
            }

            let tool_name = params.get("name").and_then(|n| n.as_str());
            let arguments = params.get("arguments").unwrap_or(&Value::Null);

            if let Some(name) = tool_name {
                match server.handle_tool_call(name, arguments).await {
                    Ok(exec_result) => Ok(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Exit code: {}\nSTDOUT:\n{}\nSTDERR:\n{}",
                                exec_result.exit_code,
                                exec_result.stdout,
                                exec_result.stderr)
                        }],
                        "isError": exec_result.exit_code != 0 && !exec_result.stopped_after,
                    })),
                    Err(e) => Err(e),
                }
            } else {
                Err(
                    genmcp::error::McpError::InvalidToolParameters("Missing tool name".to_string())
                        .into(),
                )
            }
        }
        Some("shutdown") => {
            // Check if server is initialized
            if !server.is_initialized().await {
                return Some(jsonrpc_error_response(
                    id,
                    -32000,
                    "Server not initialized. Call 'initialize' first.",
                    None,
                ));
            }

            match server.handle_shutdown().await {
                Ok(_) => Ok(Value::Null),
                Err(e) => Err(e),
            }
        }
        Some(_) | None => Err(genmcp::error::McpError::InvalidJsonRpc(format!(
            "Unknown method: {:?}",
            method
        ))
        .into()),
    };

    // Build response
    match result {
        Ok(result_value) => {
            if is_notification {
                // Notifications don't get responses
                None
            } else {
                // Build success response
                Some(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result_value,
                }))
            }
        }
        Err(e) => {
            if is_notification {
                // Notifications don't get error responses either
                None
            } else {
                // Build error response
                Some(jsonrpc_error_response(id, -32000, &e.to_string(), None))
            }
        }
    }
}

fn jsonrpc_error_response(
    id: Option<Value>,
    code: i32,
    message: &str,
    data: Option<Value>,
) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": data,
        },
    })
}
