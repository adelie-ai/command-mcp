#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// STDIN/STDOUT and WebSocket transport handlers

use crate::error::{Result, TransportError};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

/// STDIN/STDOUT transport for MCP
#[derive(Default)]
pub struct StdioTransportHandler;

impl StdioTransportHandler {
    /// Create a new STDIN/STDOUT transport handler
    #[allow(clippy::default_constructed_unit_structs)] // Default is appropriate here
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a JSON-RPC message from stdin (newline-delimited)
    pub async fn read_message(&mut self) -> Result<String> {
        let mut stdin = BufReader::new(io::stdin());
        let mut line = String::new();
        stdin.read_line(&mut line).await
            .map_err(TransportError::Io)?;
        Ok(line.trim().to_string())
    }

    /// Write a JSON-RPC message to stdout (newline-delimited)
    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        let mut stdout = io::stdout();
        stdout.write_all(message.as_bytes()).await
            .map_err(TransportError::Io)?;
        stdout.write_all(b"\n").await
            .map_err(TransportError::Io)?;
        stdout.flush().await
            .map_err(TransportError::Io)?;
        Ok(())
    }
}


/// WebSocket transport handler for MCP
pub struct WebSocketTransportHandler {
    /// JWT secret for token validation (stub: not used yet)
    _jwt_secret: Option<String>,
}

impl WebSocketTransportHandler {
    /// Create a new WebSocket transport handler
    pub fn new(jwt_secret: Option<String>) -> Self {
        Self {
            _jwt_secret: jwt_secret,
        }
    }

    /// Handle WebSocket upgrade with JWT Bearer token authentication
    pub async fn handle_upgrade(
        &self,
        ws: WebSocketUpgrade,
        headers: HeaderMap,
    ) -> std::result::Result<Response, StatusCode> {
        // Extract and validate Bearer token
        if self.extract_bearer_token(&headers).is_err() {
            return Err(StatusCode::UNAUTHORIZED);
        }

        Ok(ws.on_upgrade(Self::handle_socket))
    }

    /// Extract Bearer token from Authorization header
    fn extract_bearer_token(&self, headers: &HeaderMap) -> std::result::Result<String, TransportError> {
        let auth_header = headers.get("authorization")
            .ok_or_else(|| TransportError::Authentication("Missing Authorization header".to_string()))?
            .to_str()
            .map_err(|_| TransportError::Authentication("Invalid Authorization header".to_string()))?;

        if !auth_header.starts_with("Bearer ") {
            return Err(TransportError::Authentication("Invalid Authorization header format".to_string()));
        }

        let token = auth_header.strip_prefix("Bearer ")
            .ok_or_else(|| TransportError::Authentication("Invalid Bearer token format".to_string()))?
            .to_string();

        // Stub: Just validate that token exists and is not empty
        // Future: Validate JWT signature, expiration, etc.
        if token.is_empty() {
            return Err(TransportError::Authentication("Empty Bearer token".to_string()));
        }

        Ok(token)
    }

    /// Handle WebSocket connection
    async fn handle_socket(socket: WebSocket) {
        let (mut sender, mut receiver) = socket.split();

        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Handle JSON-RPC message
                    // TODO: Process MCP messages and route to server
                    let _ = sender.send(Message::Text(text)).await;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_stdio_transport_handler_creation() {
        let handler = StdioTransportHandler::new();
        // Just verify it can be created
        let _ = handler;
    }

    #[test]
    fn test_websocket_transport_handler_creation() {
        let handler = WebSocketTransportHandler::new(None);
        let _ = handler;
        
        let handler_with_secret = WebSocketTransportHandler::new(Some("secret".to_string()));
        let _ = handler_with_secret;
    }

    #[tokio::test]
    async fn test_extract_bearer_token_valid() {
        let handler = WebSocketTransportHandler::new(None);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer test-token-123"),
        );
        
        let token = handler.extract_bearer_token(&headers).unwrap();
        assert_eq!(token, "test-token-123");
    }

    #[tokio::test]
    async fn test_extract_bearer_token_missing_header() {
        let handler = WebSocketTransportHandler::new(None);
        let headers = HeaderMap::new();
        
        let result = handler.extract_bearer_token(&headers);
        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                TransportError::Authentication(_) => {}
                _ => panic!("Expected Authentication error"),
            }
        }
    }

    #[tokio::test]
    async fn test_extract_bearer_token_invalid_format() {
        let handler = WebSocketTransportHandler::new(None);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("InvalidFormat test-token"),
        );
        
        let result = handler.extract_bearer_token(&headers);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_extract_bearer_token_empty_token() {
        let handler = WebSocketTransportHandler::new(None);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer "),
        );
        
        let result = handler.extract_bearer_token(&headers);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_extract_bearer_token_no_bearer_prefix() {
        let handler = WebSocketTransportHandler::new(None);
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("token-123"),
        );
        
        let result = handler.extract_bearer_token(&headers);
        assert!(result.is_err());
    }
}
