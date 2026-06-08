#![deny(warnings)]

mod common;

use common::{
    minimal_echo_config_toml, pick_unused_local_port, random_secret_hex_32_bytes,
    spawn_gen_mcp_websocket, wait_for_tcp_connect, write_temp_config,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

async fn ws_connect(
    port: u16,
    authorization: Option<&str>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let mut req = url.into_client_request().expect("create websocket request");

    if let Some(auth) = authorization {
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(auth).expect("valid header value"),
        );
    }

    let (ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("websocket connect");
    ws
}

async fn ws_send_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: &Value,
) {
    let text = serde_json::to_string(msg).expect("serialize json");
    ws.send(Message::Text(text.into()))
        .await
        .expect("send ws message");
}

async fn ws_recv_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let msg = tokio::time::timeout(Duration::from_secs(3), ws.next())
        .await
        .expect("timeout waiting for ws message")
        .expect("ws stream ended")
        .expect("ws receive error");

    match msg {
        Message::Text(t) => serde_json::from_str::<Value>(&t).expect("valid json response"),
        other => panic!("unexpected websocket message: {:?}", other),
    }
}

async fn ws_request(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: &Value,
    expected_id: i64,
) -> Value {
    ws_send_json(ws, msg).await;
    let resp = ws_recv_json(ws).await;
    let id = resp
        .get("id")
        .and_then(|v| v.as_i64())
        .expect("response id should be integer");
    assert_eq!(id, expected_id);
    resp
}

async fn run_mcp_over_ws(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
        }
    });
    let init_resp = ws_request(ws, &init, 1).await;
    assert_eq!(
        init_resp
            .get("result")
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|v| v.as_str()),
        Some("2024-11-05")
    );

    // Notification (no response expected)
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    ws_send_json(ws, &initialized).await;

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "test_echo",
            "arguments": {
                "text": "hello websocket"
            }
        }
    });
    let call_resp = ws_request(ws, &call, 2).await;
    let text = call_resp
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("text"))
        .and_then(|t| t.as_str())
        .expect("tool call response text");
    assert!(
        text.contains("hello websocket"),
        "unexpected content: {}",
        text
    );
}

#[tokio::test]
async fn websocket_works_without_auth_when_jwt_not_enabled() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let port = pick_unused_local_port();
    let _server = spawn_gen_mcp_websocket(&cfg.path, "127.0.0.1", port, None);

    wait_for_tcp_connect("127.0.0.1", port, Duration::from_secs(3)).await;
    let mut ws = ws_connect(port, None).await;
    run_mcp_over_ws(&mut ws).await;
}

#[tokio::test]
async fn websocket_auth_enabled_rejects_missing_authorization_header() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let port = pick_unused_local_port();
    let secret = random_secret_hex_32_bytes();
    let _server = spawn_gen_mcp_websocket(&cfg.path, "127.0.0.1", port, Some(&secret));

    wait_for_tcp_connect("127.0.0.1", port, Duration::from_secs(3)).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let req = url.into_client_request().expect("create websocket request");
    let err = tokio_tungstenite::connect_async(req)
        .await
        .expect_err("expected 401 Unauthorized");
    let s = err.to_string();
    assert!(
        s.contains("401") || s.to_ascii_lowercase().contains("unauthorized"),
        "unexpected error: {}",
        s
    );
}

#[tokio::test]
async fn websocket_auth_enabled_accepts_valid_jwt_and_allows_end_to_end() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let port = pick_unused_local_port();
    let secret = random_secret_hex_32_bytes();
    let _server = spawn_gen_mcp_websocket(&cfg.path, "127.0.0.1", port, Some(&secret));

    wait_for_tcp_connect("127.0.0.1", port, Duration::from_secs(3)).await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs();
    let exp = now + 3600;

    let claims = serde_json::json!({
        "sub": "websocket-test",
        "exp": exp,
    });
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("encode jwt");

    let mut ws = ws_connect(port, Some(&format!("Bearer {}", token))).await;
    run_mcp_over_ws(&mut ws).await;
}

#[tokio::test]
async fn websocket_auth_enabled_rejects_bad_token() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let port = pick_unused_local_port();
    let secret = random_secret_hex_32_bytes();
    let _server = spawn_gen_mcp_websocket(&cfg.path, "127.0.0.1", port, Some(&secret));

    wait_for_tcp_connect("127.0.0.1", port, Duration::from_secs(3)).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let mut req = url.into_client_request().expect("create websocket request");
    req.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer not-a-real-jwt"),
    );

    let err = tokio_tungstenite::connect_async(req)
        .await
        .expect_err("expected 401 Unauthorized");
    let s = err.to_string();
    assert!(
        s.contains("401") || s.to_ascii_lowercase().contains("unauthorized"),
        "unexpected error: {}",
        s
    );
}
