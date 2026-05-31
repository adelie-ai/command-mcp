#![deny(warnings)]

// Binary crate for genmcp - uses library crate

use axum::{
    Router,
    extract::{State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use clap::{Parser, Subcommand, ValueEnum};
use futures_util::{SinkExt, StreamExt};
use genmcp::config::Config;
use genmcp::error::Result;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use tokio::net::TcpListener;

// Debug logging removed — was writing to a hardcoded absolute path.
// Use RUST_LOG=debug with tracing-subscriber for debug output instead.

#[derive(Clone, Debug, ValueEnum)]
enum TransportMode {
    /// STDIN/STDOUT transport (recommended for VS Code and local usage)
    Stdio,
    /// WebSocket transport (recommended for hosted MCP services)
    Websocket,
}

impl fmt::Display for TransportMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportMode::Stdio => write!(f, "stdio"),
            TransportMode::Websocket => write!(f, "websocket"),
        }
    }
}

#[derive(Parser)]
#[command(name = "genmcp")]
#[command(about = "Generic MCP Script Adapter Server")]
#[command(
    long_about = "genmcp turns existing command-line programs (scripts, binaries, and CLIs) into an MCP server.\n\nPrimary workflow:\n  1) Generate a starting config: genmcp config example > config.toml\n  2) Edit config.toml to define your tools\n  3) Run in stdio mode (VS Code): genmcp serve --config config.toml --mode stdio\n  4) Or run in websocket mode (hosted): genmcp serve --config config.toml --mode websocket --host 0.0.0.0 --port 8080\n\nTip: Use `genmcp config schema > schema.json` to view the exact config structure."
)]
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
        #[arg(short, long, env = "GENMCP_CONFIG")]
        config: String,
        /// Transport mode
        #[arg(short, long, default_value_t = TransportMode::Stdio)]
        mode: TransportMode,
        /// Port for WebSocket mode (ignored for stdio)
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        /// Host for WebSocket mode (ignored for stdio)
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
        /// JWT secret for WebSocket authentication (legacy, optional)
        #[arg(long)]
        jwt_secret: Option<String>,
        /// OIDC issuer URL for JWT validation via JWKS (preferred over jwt-secret)
        #[arg(long)]
        oidc_issuer: Option<String>,
    },
    /// Configuration helpers (schema/docs/examples)
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Output generated JSON Schema for the TOML configuration structure
    Schema,
    /// Output an example TOML configuration file
    Example,
    /// Output Markdown documentation for the configuration file format
    Docs {
        /// If set, output the curated (hand-written) docs instead of generated docs.
        ///
        /// By default, docs are generated from the Rust config structures so they stay in sync.
        #[arg(long)]
        curated: bool,
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

            match mode {
                TransportMode::Stdio => run_stdio_server(server).await?,
                TransportMode::Websocket => {
                    run_websocket_server(server, &host, port, jwt_secret, oidc_issuer).await?
                }
            }
        }
        Commands::Config { command } => match command {
            ConfigCommands::Schema => genmcp::config_schema::output_generated_schema()?,
            ConfigCommands::Example => genmcp::config_schema::output_generated_example_config()?,
            ConfigCommands::Docs { curated } => {
                if curated {
                    genmcp::config_schema::output_docs_curated()?
                } else {
                    genmcp::config_schema::output_docs_generated()?
                }
            }
        },
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
        if auth.enabled
            && let Err(e) = validate_jwt_token(&headers, auth, jwks_verifier.as_deref()).await
        {
            eprintln!("WebSocket authentication failed: {}", e);
            return (
                StatusCode::UNAUTHORIZED,
                format!("Authentication failed: {}", e),
            )
                .into_response();
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
                if let Some(resp) = response
                    && let Ok(resp_str) = serde_json::to_string(&resp)
                    && let Err(e) = sender.send(Message::Text(resp_str.into())).await
                {
                    eprintln!("Error sending WebSocket response: {}", e);
                    break;
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
                    Ok(exec_result) => {
                        // Always include STDERR in the response, even if empty
                        let mut response_text = format!(
                            "Exit code: {}\n\nSTDOUT:\n{}",
                            exec_result.exit_code, exec_result.stdout
                        );

                        // Always show STDERR section, even if empty
                        if exec_result.stderr.is_empty() {
                            response_text.push_str("\n\nSTDERR:\n(no output)");
                        } else {
                            response_text.push_str(&format!("\n\nSTDERR:\n{}", exec_result.stderr));
                        }

                        Ok(serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": response_text
                            }],
                            "isError": exec_result.exit_code != 0 && !exec_result.stopped_after,
                        }))
                    }
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
