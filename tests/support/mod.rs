//! A capturing `tracing` layer, and a driver that runs command-mcp's real
//! service under it.
//!
//! The telemetry criteria are about what the dispatch and execution paths
//! emit, so a test has to read the spans and events back rather than assert
//! a constant against itself. Each test file that needs this gets its own
//! copy via `mod support;`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use command_mcp::config::Config;
use command_mcp::service::CommandMcpService;
use mcp_core::{ServerConfig, ServerCore, Session};
use serde_json::{Value, json};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

/// One span, as the subscriber saw it. A span whose fields are recorded
/// after creation appears a second time, carrying only what was recorded
/// then.
#[derive(Clone, Debug)]
pub struct RecordedSpan {
    /// The span's name.
    pub name: &'static str,
    /// Field name to its rendered value.
    pub fields: BTreeMap<String, String>,
}

/// One event, as the subscriber saw it.
#[derive(Clone, Debug)]
pub struct RecordedEvent {
    /// The level the event was emitted at.
    pub level: tracing::Level,
    /// Field name to its rendered value. The message is the `message` field.
    pub fields: BTreeMap<String, String>,
}

/// Everything one captured run produced.
#[derive(Clone, Debug, Default)]
pub struct Recorded {
    /// Spans, in the order they opened (or were re-recorded).
    pub spans: Vec<RecordedSpan>,
    /// Events, in the order they were emitted.
    pub events: Vec<RecordedEvent>,
}

impl Recorded {
    /// A short rendering for an assertion message.
    pub fn span_summary(&self) -> Vec<String> {
        self.spans
            .iter()
            .map(|span| format!("{}{:?}", span.name, span.fields))
            .collect()
    }

    /// A short rendering for an assertion message.
    pub fn event_summary(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|event| format!("{}{:?}", event.level, event.fields))
            .collect()
    }
}

/// Run `body` with a capturing subscriber installed on this thread, and
/// return what it emitted.
pub fn capture<F, Fut>(body: F) -> Recorded
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    tracing::subscriber::with_default(subscriber, || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime");
        runtime.block_on(body());
    });
    capture.take()
}

/// A shared core built from `config_toml`. Fresh every call, so state from
/// one test never leaks into another.
pub fn demo_core(config_toml: &str) -> Arc<ServerCore> {
    let config = Config::from_str(config_toml).expect("parse test config toml");
    let service = CommandMcpService::new(config).expect("build service from config");
    let server_config = ServerConfig::new("command-mcp", env!("CARGO_PKG_VERSION"))
        .instructions("command-mcp telemetry acceptance test harness");
    ServerCore::new(server_config, Arc::new(service))
}

/// Drive `messages` through one session over the real service built from
/// `config_toml`, capturing what the dispatch and execution paths emitted.
pub fn capture_dispatch(config_toml: &str, messages: &[Value]) -> Recorded {
    let config_toml = config_toml.to_string();
    let messages = messages.to_vec();
    capture(|| async move {
        let mut session = Session::new(demo_core(&config_toml));
        for message in messages {
            session.handle_message(message).await;
        }
    })
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Recorded>>);

impl Capture {
    fn take(self) -> Recorded {
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .clone()
    }
}

impl<S> Layer<S> for Capture
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan {
                name: attrs.metadata().name(),
                fields,
            });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let name = ctx.span(id).map_or("<closed>", |span| span.name());
        let mut fields = BTreeMap::new();
        values.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .spans
            .push(RecordedSpan { name, fields });
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = BTreeMap::new();
        event.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("the capture lock is only held to push one record")
            .events
            .push(RecordedEvent {
                level: *event.metadata().level(),
                fields,
            });
    }
}

struct Collector<'a>(&'a mut BTreeMap<String, String>);

impl Visit for Collector<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

// --- The content-leak sweep (mcp-core#40 lesson 8) -------------------------
//
// command-mcp has one generic tool handler rather than one Rust function per
// tool (the tool set is config-driven), so the equivalent of "one tool out
// of twenty-seven" is one *parameter kind or output mode* out of the several
// the config format supports. A leak test proved against a single plain
// positional argument proves the mechanism works and nothing about a
// `split_args` value, a flag's value, structured JSON output, a second
// positional parameter, a failing command, or a leak through the operator's
// own configured command rather than a caller argument.
//
// `sweep_config` and `cases` are two views of the same tool list, and
// `every_configured_tool_has_a_sweep_case` (in `telemetry_span_fields.rs`)
// keeps them from drifting apart: a tool added to `sweep_config` without a
// matching `Case` fails that test by name instead of running uncovered.
//
// Covering every tool is not covering every path (mcp-core#40 lesson 9): a
// leak most naturally appears in an error's `Display`, written to be
// helpful, and helpful means quoting what failed. So the sweep drives
// failure branches too, not only success: `always_fails` (a nonzero exit),
// `missing_command` (a spawn failure whose message embeds the *configured*
// command), and `malformed_split_args` (a `parse_shell_args` parse failure
// whose message embeds the *caller's* raw string verbatim -- the closest
// analogue here to a Display that quotes a failed request). The rejection
// in each case happens inside `CommandMcpService::execute_tool_call`, the
// method under test, not in a guard ahead of it, so each still opens
// `command_mcp.call_tool` -- checked by `reaches_execute` below and by
// `tool_call_records_no_arguments`'s span-count assertions.
//
// Axes swept: every `tracing::`/`#[instrument]` call site in `src/`
// (production code, 2 total, both listed in this module's own doc comment
// history), every `#[error(...)]` Display template in `error.rs`, and every
// `{:?}` in `src/` (one, test-only). Not swept: `McpError::OverrideExceedsMax`
// (embeds only a fixed field name and two numbers, no caller string) and the
// `Timeout`/`CommandFailed` `ExecutionError` variants (same "embeds the
// configured command" shape `missing_command` already covers, just reached
// more slowly -- a real multi-second wait each). Command-mcp has no upstream
// API to mock, so there is no "mock always succeeds" axis to name here.

/// One tool in the sweep and the sentinel(s) its call must surface -- at
/// DEBUG/TRACE only, never in a span field or an INFO-or-louder event field.
pub struct Case {
    /// The full `{group}_{tool}` name, matching a `[[groups.sweep.tools]]`
    /// entry in [`sweep_config`].
    pub tool: &'static str,
    /// The `tools/call` `arguments` object to send.
    pub arguments: Value,
    /// Every sentinel this call should surface somewhere at DEBUG/TRACE.
    /// More than one entry only for the case that exercises two positional
    /// parameters at once.
    pub sentinels: Vec<String>,
    /// Whether this call is expected to reach `executor::execute_command`
    /// (and so open a `command.execute` span). `false` only for
    /// `malformed_split_args`, which is rejected by `parse_shell_args`
    /// before a command is ever built.
    pub reaches_execute: bool,
}

/// A sentinel unique to `tag`, so a leak in an assertion message names the
/// exact case that produced it.
fn sentinel(tag: &str) -> String {
    format!("MARKER-command-mcp-{tag}-9f3d1c2a")
}

/// The sentinel embedded directly in `sweep_config`'s `missing_command`
/// tool. Fixed (not derived from [`sentinel`]) because it has to match a
/// literal in the TOML below.
pub const MISSING_COMMAND_SENTINEL: &str = "MARKER-command-mcp-missing-command-9f3d1c2a";

/// The TOML config for the sweep: one tool per parameter kind / output mode,
/// plus a command that does not exist (the configured-command leak axis)
/// and a command that always fails (the nonzero-exit reply path).
pub fn sweep_config() -> &'static str {
    r#"
[groups.sweep]
default_timeout = 5
default_termination_grace_period = 1

  [[groups.sweep.tools]]
  name = "positional"
  description = "a plain positional argument, not split"
  command = "/bin/echo"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true

  [[groups.sweep.tools]]
  name = "split_args"
  description = "a shell-style split positional argument"
  command = "/bin/echo"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true
    split_args = true

  [[groups.sweep.tools]]
  name = "flag_value"
  description = "a flag that takes a value"
  command = "/bin/echo"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true
    flag = "--value"
    takes_value = true

  [[groups.sweep.tools]]
  name = "json_output"
  description = "structured JSON output"
  command = "/bin/echo"
  output = "json"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true

  [[groups.sweep.tools]]
  name = "multi_param"
  description = "two positional parameters via arg_order"
  command = "/bin/echo"
  arg_order = ["first", "second"]
    [groups.sweep.tools.parameters.first]
    description = "first"
    required = true
    [groups.sweep.tools.parameters.second]
    description = "second"
    required = true

  [[groups.sweep.tools]]
  name = "always_fails"
  description = "a command that always exits nonzero"
  command = "/bin/false"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true

  [[groups.sweep.tools]]
  name = "missing_command"
  description = "a command that does not exist"
  command = "/nonexistent/MARKER-command-mcp-missing-command-9f3d1c2a"

  [[groups.sweep.tools]]
  name = "malformed_split_args"
  description = "a split_args value with an unclosed quote"
  command = "/bin/echo"
  arg_order = ["value"]
    [groups.sweep.tools.parameters.value]
    description = "value"
    required = true
    split_args = true
"#
}

/// One [`Case`] per tool declared in [`sweep_config`]. Add a tool there,
/// add its case here -- `every_configured_tool_has_a_sweep_case` fails
/// loudly if the two lists drift apart.
pub fn cases() -> Vec<Case> {
    let positional = sentinel("positional");
    let split_args = sentinel("split-args");
    let flag_value = sentinel("flag-value");
    let json_value = sentinel("json-output");
    let multi_first = sentinel("multi-param-first");
    let multi_second = sentinel("multi-param-second");
    let always_fails = sentinel("always-fails");
    let malformed_split_args = sentinel("malformed-split-args");

    vec![
        Case {
            tool: "sweep_positional",
            arguments: json!({"value": positional}),
            sentinels: vec![positional],
            reaches_execute: true,
        },
        Case {
            tool: "sweep_split_args",
            arguments: json!({"value": format!("{split_args} more words")}),
            sentinels: vec![split_args],
            reaches_execute: true,
        },
        Case {
            tool: "sweep_flag_value",
            arguments: json!({"value": flag_value}),
            sentinels: vec![flag_value],
            reaches_execute: true,
        },
        Case {
            tool: "sweep_json_output",
            arguments: json!({"value": format!(r#"{{"marker":"{json_value}"}}"#)}),
            sentinels: vec![json_value],
            reaches_execute: true,
        },
        Case {
            tool: "sweep_multi_param",
            arguments: json!({"first": multi_first, "second": multi_second}),
            sentinels: vec![multi_first, multi_second],
            reaches_execute: true,
        },
        Case {
            // A failure branch, not a success (mcp-core#40 lesson 9): the
            // command runs and exits nonzero, so the reply is a successful
            // `isError: true` `ToolReply`, not a `CallError` -- a different
            // code path than every case above.
            tool: "sweep_always_fails",
            arguments: json!({"value": always_fails}),
            sentinels: vec![always_fails],
            reaches_execute: true,
        },
        Case {
            // A failure branch through the *configured* command rather than
            // caller input: the leak axis here is the operator's own
            // `sweep_config` entry (embedded above), not `arguments`, so
            // this case sends none.
            tool: "sweep_missing_command",
            arguments: json!({}),
            sentinels: vec![MISSING_COMMAND_SENTINEL.to_string()],
            reaches_execute: true,
        },
        Case {
            // A failure branch through `parse_shell_args` itself: an
            // unclosed quote is rejected with a message that embeds the
            // caller's raw string verbatim (`error.rs`'s
            // `"Unclosed quote in arguments: {}"`), the closest analogue
            // here to the homeassistant-mcp finding that started
            // mcp-core#40 lesson 9 (an error `Display` quoting a failed
            // request). Rejected inside `execute_tool_call` before
            // `execute_command` is ever called, so `reaches_execute` is
            // `false`.
            tool: "sweep_malformed_split_args",
            arguments: json!({"value": format!("{malformed_split_args} 'unclosed")}),
            sentinels: vec![malformed_split_args],
            reaches_execute: false,
        },
    ]
}
