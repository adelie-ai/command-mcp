#![deny(warnings)]

//! Real-process telemetry acceptance tests (mcp-core#40): what a
//! default-feature build resolves, what actually reaches stdout, and what
//! reaches stderr at each log level.
//!
//! `telemetry_span_fields.rs` covers the half of the leak surface a
//! console-text test cannot see -- `#[tracing::instrument]` capturing an
//! argument into a span field, which nothing here prints unless an event
//! fires inside that span (mcp-core#40, lesson 7). This file is the console
//! half: it catches events and a stray `println!`/`eprintln!` on the stdio
//! transport.
//!
//! Both level-contract tests below drive `support::sweep_config()` /
//! `support::cases()` -- every parameter kind and output mode command-mcp's
//! generic argument pipeline supports, not one tool (mcp-core#40 lesson 8).

mod common;
mod support;

use common::{command_mcp_bin, write_temp_config};
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Spawn `command-mcp serve --config <config_path> --mode stdio` with `env`,
/// write `requests` (newline-delimited JSON-RPC) to stdin, close stdin, and
/// return everything the process wrote to stdout and to stderr.
///
/// Both pipes are drained concurrently from the moment the child starts, so
/// a chatty `RUST_LOG=trace` run can never fill a pipe buffer and deadlock
/// against this function still writing requests.
async fn run_and_capture(
    config_path: &Path,
    env: &[(&str, &str)],
    requests: &[Value],
) -> (String, String) {
    let mut cmd = Command::new(command_mcp_bin());
    cmd.arg("serve")
        .arg("--config")
        .arg(config_path)
        .arg("--mode")
        .arg("stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }

    let mut child = cmd.spawn().expect("spawn command-mcp serve --mode stdio");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = child.stdout.take().expect("child stdout");
    let mut stderr = child.stderr.take().expect("child stderr");

    let stdout_reader = tokio::spawn(async move {
        let mut buf = String::new();
        stdout
            .read_to_string(&mut buf)
            .await
            .expect("read child stdout");
        buf
    });
    let stderr_reader = tokio::spawn(async move {
        let mut buf = String::new();
        stderr
            .read_to_string(&mut buf)
            .await
            .expect("read child stderr");
        buf
    });

    for request in requests {
        let line = serde_json::to_string(request).expect("serialize jsonrpc");
        stdin
            .write_all(line.as_bytes())
            .await
            .expect("write jsonrpc line");
        stdin.write_all(b"\n").await.expect("write newline");
        stdin.flush().await.expect("flush stdin");
    }
    drop(stdin);

    let stdout_captured = stdout_reader.await.expect("stdout reader task");
    let stderr_captured = stderr_reader.await.expect("stderr reader task");
    let status = child.wait().await.expect("wait for child");
    assert!(
        status.success(),
        "command-mcp must exit cleanly on stdin EOF, got: {status:?}\nstderr:\n{stderr_captured}"
    );

    (stdout_captured, stderr_captured)
}

/// AC (epic AC2): a default-feature build resolves no `opentelemetry*`
/// crate. The `otel` feature is the only thing that adds one, and a
/// stdio-only server that never turns it on must not compile one in.
#[test]
fn default_build_pulls_no_opentelemetry() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    let output = std::process::Command::new(cargo)
        .args(["tree", "--edges", "normal", "--prefix", "none", "--locked"])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .expect("cargo tree must run");

    assert!(
        output.status.success(),
        "cargo tree failed, so this criterion is unproven: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = tree
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_lowercase().starts_with("opentelemetry"))
        .collect();

    assert!(
        found.is_empty(),
        "a default-feature build must resolve no opentelemetry crate, but it resolved: {found:?}"
    );
}

/// Build the `initialize` handshake plus one `tools/call` per
/// [`support::Case`] in the sweep, assigning sequential ids.
fn sweep_requests(cases: &[support::Case]) -> Vec<Value> {
    let mut requests = vec![serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    })];
    for (offset, case) in cases.iter().enumerate() {
        requests.push(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2 + offset as u64,
            "method": "tools/call",
            "params": {"name": case.tool, "arguments": case.arguments},
        }));
    }
    requests
}

/// AC (mcp-core#40 non-negotiable #3 / epic AC4): with `RUST_LOG=trace`,
/// every line command-mcp writes to stdout parses as JSON-RPC, and the logs
/// land on stderr instead. This server speaks stdio; one log line on stdout
/// corrupts the protocol stream. Driven by the whole sweep (mcp-core#40
/// lesson 8), not one tool, so a future parameter kind or output mode that
/// writes to stdout directly (rather than through the framed response) is
/// exercised here too.
#[tokio::test]
async fn stdout_carries_only_jsonrpc_at_trace_log_level() {
    let cfg = write_temp_config(support::sweep_config());
    let cases = support::cases();
    let requests = sweep_requests(&cases);

    let (stdout, stderr) = run_and_capture(&cfg.path, &[("RUST_LOG", "trace")], &requests).await;

    let mut replies = 0;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("every stdout line must be JSON-RPC at RUST_LOG=trace: {e}\nline: {line:?}")
        });
        assert_eq!(
            value.get("jsonrpc").and_then(Value::as_str),
            Some("2.0"),
            "every stdout line must carry the JSON-RPC envelope: {line:?}"
        );
        replies += 1;
    }
    assert_eq!(
        replies,
        requests.len(),
        "command-mcp must answer every request in the sweep"
    );

    assert!(
        stderr.contains("INFO") || stderr.contains("DEBUG") || stderr.contains("TRACE"),
        "at RUST_LOG=trace the logs must arrive on stderr, or the subscriber was never \
         installed. stderr was: {stderr:?}"
    );
}

/// AC (mcp-core#40, epic D10): at the default log level (`info`), no
/// sentinel from any case in the sweep reaches stderr. The `RUST_LOG=trace`
/// run alongside it is a positive control per sentinel: it proves each one
/// really does surface somewhere in this harness's capture (mcp-core logs
/// tool arguments at DEBUG, and a declined tool call's message at DEBUG), so
/// the `RUST_LOG=info` absence assertion is not passing because nothing was
/// ever captured. Swept over every case (mcp-core#40 lesson 8): a leak that
/// only a `split_args` value, a flag's value, structured JSON output, a
/// second positional parameter, or the configured command itself (not a
/// caller argument -- the `missing_command` case) would produce is not
/// something a single-tool console test can see.
#[tokio::test]
async fn tool_call_console_carries_no_sentinel_at_info_level() {
    let cfg = write_temp_config(support::sweep_config());
    let cases = support::cases();
    let requests = sweep_requests(&cases);

    let (_stdout, info_stderr) =
        run_and_capture(&cfg.path, &[("RUST_LOG", "info")], &requests).await;
    let (_stdout2, trace_stderr) =
        run_and_capture(&cfg.path, &[("RUST_LOG", "trace")], &requests).await;

    for case in &cases {
        for sentinel in &case.sentinels {
            assert!(
                !info_stderr.contains(sentinel.as_str()),
                "tool {:?}: sentinel {sentinel:?} reached stderr at RUST_LOG=info:\n{info_stderr}",
                case.tool
            );
            assert!(
                trace_stderr.contains(sentinel.as_str()),
                "tool {:?}: expected sentinel {sentinel:?} to surface at RUST_LOG=trace as a \
                 positive control (tool arguments are DEBUG-level content per D10); got none, \
                 so the RUST_LOG=info assertion above is not meaningful for this case:\n{trace_stderr}",
                case.tool
            );
        }
    }
}
