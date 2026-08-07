#![deny(warnings)]

//! Metrics acceptance tests (mcp-core#40): `command.execute`, a counter
//! labelled `tool` and `outcome`, and `command.execute.duration`, a
//! histogram labelled `tool`.
//!
//! Both distinguish outcomes mcp-core's own protocol-level `mcp.tools.call`
//! metric cannot see: a nonzero shell exit and a `stop_after`-terminated
//! command are both a *successful* JSON-RPC call (see `service::call_tool`),
//! so mcp-core's own outcome label sees each as `ok`.

mod support;

use mcp_core::telemetry::metrics::{self, Label};
use serde_json::json;
use support::capture_dispatch;

/// The metrics registry [`mcp_core::telemetry::metrics`] records into is
/// process-global, and `cargo test` runs a file's tests concurrently by
/// default. Every test below records into and reads the registry, so two
/// tests running at once can inflate each other's before/after delta. This
/// guards every test in the file so they run one at a time relative to each
/// other; it holds no data of its own. (mcp-core#40 lesson 6,
/// adelie-telemetry#6)
static METRICS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_metrics() -> std::sync::MutexGuard<'static, ()> {
    METRICS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn test_config() -> &'static str {
    r#"
[groups.test]
default_timeout = 10
default_termination_grace_period = 1

  [[groups.test.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
  arg_order = ["text"]
    [groups.test.tools.parameters.text]
    description = "text"
    required = true

  [[groups.test.tools]]
  name = "false"
  description = "Always exits 1"
  command = "/bin/false"

  [[groups.test.tools]]
  # Ignores SIGTERM (a `trap` in its own script), so a `stop_after` call
  # against it always takes the force-kill (SIGKILL) path deterministically.
  # A plain SIGTERM-killable command (e.g. `/bin/sleep`) races `child.wait()`
  # against the stop_after task's own grace-period sleep in
  # `executor::handle_stop_after`, and `child.wait()` wins as soon as the
  # signal kills the child -- well before that sleep finishes -- so
  # `stopped_after` comes back `false` even though `stop_after` caused the
  # termination. `executor::tests::test_stop_after_feature` documents this
  # same race and deliberately does not assert on `stopped_after` for that
  # reason. Filed as adelie-ai/command-mcp#24; out of scope here.
  name = "stubborn"
  description = "Sleeps 10s, ignoring SIGTERM"
  command = "/bin/sh"
  arg_order = ["script"]
    [groups.test.tools.parameters.script]
    description = "shell script"
    required = true
    split_args = true
"#
}

fn counter_total(name: &str, labels: &[Label]) -> u64 {
    metrics::global()
        .snapshot()
        .counters
        .iter()
        .find(|counter| counter.name == name && same_labels(&counter.labels, labels))
        .map_or(0, |counter| counter.total)
}

fn histogram_count(name: &str, labels: &[Label]) -> u64 {
    metrics::global()
        .snapshot()
        .histograms
        .iter()
        .find(|histogram| histogram.name == name && same_labels(&histogram.labels, labels))
        .map_or(0, |histogram| histogram.total.count)
}

fn same_labels(recorded: &[Label], wanted: &[Label]) -> bool {
    recorded.len() == wanted.len()
        && wanted.iter().all(|want| {
            recorded
                .iter()
                .any(|have| have.key() == want.key() && have.value() == want.value())
        })
}

/// AC (mcp-core#40): a successful command increments `command.execute`
/// labelled `tool=test_echo, outcome=ok`, and records its latency into
/// `command.execute.duration` labelled `tool=test_echo`.
#[test]
fn command_execute_records_ok_outcome_metric_labelled_by_tool() {
    let _guard = lock_metrics();
    let ok_labels = [Label::new("tool", "test_echo"), Label::new("outcome", "ok")];
    let tool_label = [Label::new("tool", "test_echo")];
    let calls_before = counter_total("command.execute", &ok_labels);
    let duration_before = histogram_count("command.execute.duration", &tool_label);

    capture_dispatch(
        test_config(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "test_echo", "arguments": {"text": "hi"}},
            }),
        ],
    );

    assert_eq!(
        counter_total("command.execute", &ok_labels),
        calls_before + 1,
        "a successful command must increment command.execute labelled tool=test_echo, outcome=ok"
    );
    assert!(
        histogram_count("command.execute.duration", &tool_label) > duration_before,
        "a completed execution must record its latency into command.execute.duration"
    );
}

/// AC (mcp-core#40): a nonzero exit is counted under its own bounded
/// outcome, distinct from a genuine execution failure -- the JSON-RPC call
/// itself still succeeds (a non-error `ToolReply` with `isError: true`
/// content), which is domain information mcp-core's own protocol-level
/// outcome metric cannot see.
#[test]
fn command_execute_records_nonzero_exit_outcome_metric() {
    let _guard = lock_metrics();
    let labels = [
        Label::new("tool", "test_false"),
        Label::new("outcome", "nonzero_exit"),
    ];
    let before = counter_total("command.execute", &labels);

    capture_dispatch(
        test_config(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "test_false", "arguments": {}},
            }),
        ],
    );

    assert_eq!(
        counter_total("command.execute", &labels),
        before + 1,
        "a nonzero exit must be counted under its own bounded outcome label"
    );
}

/// AC (mcp-core#40): a command stopped by the `stop_after` budget is counted
/// as `stopped_after`, distinct from both `ok` and `nonzero_exit`. Unique to
/// command-mcp among the fleet (the `stop_after` domain concept), and the
/// clearest case of an outcome mcp-core's protocol-level metric cannot see:
/// the call still returns a successful, non-error reply (see
/// `service::call_tool`).
///
/// Drives the SIGTERM-ignoring `stubborn` tool, not a plain command: see its
/// comment in `test_config` for why a command that dies on the first signal
/// cannot reliably reach `stopped_after: true` today (command-mcp#24).
#[test]
fn command_execute_records_stopped_after_outcome_metric() {
    let _guard = lock_metrics();
    let labels = [
        Label::new("tool", "test_stubborn"),
        Label::new("outcome", "stopped_after"),
    ];
    let before = counter_total("command.execute", &labels);

    capture_dispatch(
        test_config(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {
                    "name": "test_stubborn",
                    "arguments": {
                        "script": "-c \"trap '' TERM; sleep 10; :\"",
                        "stop_after": 1,
                    },
                },
            }),
        ],
    );

    assert_eq!(
        counter_total("command.execute", &labels),
        before + 1,
        "a stop_after-terminated command must be counted under its own bounded outcome label"
    );
}
