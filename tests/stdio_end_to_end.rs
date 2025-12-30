#![deny(warnings)]

mod common;

use common::{minimal_echo_config_toml, spawn_genmcp_stdio, write_temp_config};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn write_json_line(stdin: &mut tokio::process::ChildStdin, msg: &Value) {
    let s = serde_json::to_string(msg).expect("serialize json");
    stdin
        .write_all(format!("{}\n", s).as_bytes())
        .await
        .expect("write stdin");
    stdin.flush().await.expect("flush stdin");
}

async fn read_json_line(stdout: &mut BufReader<tokio::process::ChildStdout>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), stdout.read_line(&mut line))
        .await
        .expect("timeout reading stdout")
        .expect("read stdout");
    assert!(!line.trim().is_empty(), "expected non-empty stdout line");
    serde_json::from_str::<Value>(line.trim()).expect("parse json response")
}

async fn read_response_for_id(
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    expected_id: i64,
) -> Value {
    let v = read_json_line(stdout).await;
    let id = v
        .get("id")
        .and_then(|v| v.as_i64())
        .expect("response id should be integer");
    assert_eq!(id, expected_id);
    v
}

#[tokio::test]
async fn stdio_end_to_end_initialize_and_tool_call() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let mut server = spawn_genmcp_stdio(&cfg.path);

    let mut stdin = server.child.stdin.take().expect("child stdin");
    let stdout = server.child.stdout.take().expect("child stdout");
    let mut stdout = BufReader::new(stdout);

    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
        }
    });
    write_json_line(&mut stdin, &init).await;
    let init_resp = read_response_for_id(&mut stdout, 1).await;
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
    write_json_line(&mut stdin, &initialized).await;

    let call = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "test_echo",
            "arguments": {
                "text": "hello stdio"
            }
        }
    });
    write_json_line(&mut stdin, &call).await;
    let call_resp = read_response_for_id(&mut stdout, 2).await;
    let text = call_resp
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("text"))
        .and_then(|t| t.as_str())
        .expect("tool call response text");
    assert!(text.contains("hello stdio"), "unexpected content: {}", text);
}


