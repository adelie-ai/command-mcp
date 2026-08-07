#![deny(warnings)]

//! Span-field acceptance tests (mcp-core#40).
//!
//! `telemetry_console.rs` reads the console text a real process writes to
//! stderr. That misses one class of leak: `#[tracing::instrument]` captures
//! a function's arguments into the span's *fields* the moment the span is
//! created, whether or not any event ever renders that span on the console.
//! With the `otel` feature and a collector attached, a span exports on close
//! with every field it holds, independent of local console output. A
//! console-only test cannot see that path, so this one drives the dispatch
//! in process under a capturing `tracing` layer and reads the raw spans and
//! events back directly.
//!
//! Both tests below use `support::sweep_config()` / `support::cases()`:
//! every parameter kind and output mode command-mcp's generic argument
//! pipeline supports, not one tool. A leak test proved against a single
//! plain positional argument proves the mechanism works and nothing about a
//! branch that argument never reaches (mcp-core#40 lesson 8).

mod support;

use mcp_core::McpService;
use std::collections::BTreeSet;
use support::capture_dispatch;
use tracing::Level;

/// AC (mcp-core#40, epic D10): `tool_call_records_no_arguments`. Neither a
/// caller-supplied argument nor a leak through the operator's own configured
/// command reaches a span field or an INFO-or-louder event field, for every
/// case in the sweep. The same run proves the positive half too: each case
/// opens command-mcp's own `command_mcp.call_tool` and `command.execute`
/// spans (so this cannot pass simply because nothing was instrumented), and
/// each sentinel really does surface at DEBUG/TRACE (mcp-core's inherited
/// argument logging, or its declined-tool-call logging for the
/// `missing_command` case), so the absence assertion is not vacuous.
#[test]
fn tool_call_records_no_arguments() {
    let cases = support::cases();

    let mut messages = vec![serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    })];
    for (offset, case) in cases.iter().enumerate() {
        messages.push(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2 + offset as u64,
            "method": "tools/call",
            "params": {"name": case.tool, "arguments": case.arguments},
        }));
    }

    let recorded = capture_dispatch(support::sweep_config(), &messages);

    let call_tool_spans = recorded
        .spans
        .iter()
        .filter(|s| s.name == "command_mcp.call_tool")
        .count();
    assert_eq!(
        call_tool_spans,
        cases.len(),
        "expected one command_mcp.call_tool span per case in the sweep; spans were {:?}",
        recorded.span_summary()
    );
    let execute_spans = recorded
        .spans
        .iter()
        .filter(|s| s.name == "command.execute")
        .count();
    let expected_execute_spans = cases.iter().filter(|c| c.reaches_execute).count();
    assert_eq!(
        execute_spans,
        expected_execute_spans,
        "expected one command.execute span per case with reaches_execute = true; spans were {:?}",
        recorded.span_summary()
    );

    for case in &cases {
        for sentinel in &case.sentinels {
            for span in &recorded.spans {
                for (key, value) in &span.fields {
                    assert!(
                        !value.contains(sentinel.as_str()),
                        "tool {:?}: sentinel {sentinel:?} leaked into span {:?} field {key:?}: \
                         {value:?}; all spans were {:?}",
                        case.tool,
                        span.name,
                        recorded.span_summary()
                    );
                }
            }

            let mut saw_below_info = false;
            for event in &recorded.events {
                for (key, value) in &event.fields {
                    if !value.contains(sentinel.as_str()) {
                        continue;
                    }
                    if event.level == Level::DEBUG || event.level == Level::TRACE {
                        saw_below_info = true;
                        continue;
                    }
                    panic!(
                        "tool {:?}: sentinel {sentinel:?} leaked into an INFO-or-louder event \
                         field {key:?}: {value:?}; all events were {:?}",
                        case.tool,
                        recorded.event_summary()
                    );
                }
            }
            assert!(
                saw_below_info,
                "tool {:?}: expected sentinel {sentinel:?} to surface at DEBUG/TRACE as a \
                 positive control; seeing none means this case's absence assertion above is \
                 not meaningful",
                case.tool
            );
        }
    }
}

/// Structural guarantee for mcp-core#40 lesson 8: a tool added to
/// `support::sweep_config` without a matching entry in `support::cases`
/// fails this test by name, rather than running uncovered while both
/// content tests stay green. Compares against what the built service
/// actually advertises via `tools()`, not a second hand-kept copy of
/// `sweep_config`'s tool names.
#[test]
fn every_configured_tool_has_a_sweep_case() {
    let config =
        command_mcp::config::Config::from_str(support::sweep_config()).expect("parse sweep config");
    let service =
        command_mcp::service::CommandMcpService::new(config).expect("build sweep service");

    let configured: BTreeSet<String> = service.tools().into_iter().map(|t| t.name).collect();
    let covered: BTreeSet<String> = support::cases()
        .into_iter()
        .map(|case| case.tool.to_string())
        .collect();

    assert_eq!(
        configured,
        covered,
        "sweep_config's tools and cases()'s sentinel coverage have drifted: \
         configured-but-uncovered = {:?}, covered-but-not-configured = {:?}",
        configured.difference(&covered).collect::<Vec<_>>(),
        covered.difference(&configured).collect::<Vec<_>>(),
    );
}
