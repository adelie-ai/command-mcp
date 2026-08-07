#![deny(warnings)]

//! Acceptance criteria for what command-mcp does when a signal stops it
//! (adelie-ai/command-mcp#26).
//!
//! command-mcp's `serve` subcommand lives alongside a `config` subcommand, so
//! `main` drives `mcp_core::serve` directly rather than `mcp_core::run`.
//! `run` gained signal handling in adelie-ai/mcp-core#46 and eleven of the
//! thirteen MCP servers inherited it for free; command-mcp did not, because
//! it never reaches `run`.
//!
//! Every test here drives the real binary, spawns it, signals it, and reads
//! its real stderr. An in-process test can prove what the telemetry guard
//! does when it is dropped. Only a real process, stopped by the operating
//! system, proves it got as far as dropping it -- which is the whole
//! question this ticket asks.
//!
//! Each test is named after the criterion it holds, so a failing run names
//! the unmet requirement rather than a line number.

mod common;

use common::{
    command_mcp_bin, minimal_echo_config_toml, pick_unused_local_port, write_temp_config,
};
use futures_util::{SinkExt, StreamExt};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

/// How long a signalled server may take to stop before the test gives up.
/// The flush itself is bounded by the telemetry guard's own shutdown budget,
/// five seconds by default, so anything past this is a hang rather than a
/// slow collector.
const STOP_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to wait for the websocket probe to report it is listening.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// The line the telemetry guard writes when it closes the last window. Its
/// absence is exactly the loss adelie-ai/command-mcp#26 is about.
const SUMMARY: &str = "metrics summary";

/// AC: a server stopped by `SIGTERM` over stdio flushes its telemetry.
#[test]
fn sigterm_over_stdio_flushes_the_final_metrics_summary() {
    let stderr = stdio_probe_stopped_by("TERM").stderr;
    assert!(
        stderr.contains(SUMMARY),
        "a SIGTERM over stdio must still write the final metrics summary, \
         but stderr was: {stderr:?}"
    );
}

/// AC: `SIGINT` behaves the same way as `SIGTERM`.
#[test]
fn sigint_over_stdio_flushes_the_final_metrics_summary() {
    let stderr = stdio_probe_stopped_by("INT").stderr;
    assert!(
        stderr.contains(SUMMARY),
        "a SIGINT must be treated exactly as a SIGTERM, but stderr was: {stderr:?}"
    );
}

/// AC: the flushed summary carries the numbers the run really recorded, not
/// an empty shell of a summary.
#[test]
fn the_flushed_summary_carries_the_counters_the_run_recorded() {
    let stderr = stdio_probe_stopped_by("TERM").stderr;
    assert!(
        stderr.contains("mcp.requests"),
        "the flushed summary must carry the request counter the run recorded, \
         but stderr was: {stderr:?}"
    );
}

/// AC: a signalled server reports a clean exit status rather than dying by
/// the signal. `code()` is `None` for a process killed by a signal, so this
/// distinguishes "handled the signal and returned" from "was killed by it".
#[test]
fn a_signalled_server_exits_zero_rather_than_dying_by_signal() {
    let status = stdio_probe_stopped_by("TERM").status;
    assert_eq!(
        status.code(),
        Some(0),
        "a signalled server must exit 0, not die by the signal; status was {status:?}"
    );
}

/// AC: the stop path writes nothing to stdout. The stdio transport frames
/// JSON-RPC there, and one stray line from a signal handler corrupts the
/// stream for a client that is still reading it.
#[test]
fn the_stop_path_writes_nothing_to_stdout() {
    // `request` already read the one reply the server owed, so everything
    // here is what the stop path added. It has to be nothing at all: a
    // stray line that happened to parse as JSON would corrupt the stream
    // just as surely as one that did not.
    let stopped = stdio_probe_stopped_by("TERM");
    assert!(
        stopped.stdout.is_empty(),
        "the stop path must write nothing to stdout, but it wrote: {:?}",
        stopped.stdout
    );
}

/// AC: two signals in quick succession neither panic nor flush twice.
///
/// Whether the second signal lands during the flush or just after it depends
/// on how long the flush takes, and with no collector configured that is a
/// fraction of a millisecond. This test does not control which of the two
/// happens, so it asserts what must hold either way: the process still exits
/// 0, nothing panics, and exactly one summary is written.
#[test]
fn a_second_signal_during_shutdown_neither_panics_nor_double_flushes() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let mut probe = Probe::start_stdio(&cfg.path);
    probe.request(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);

    probe.signal("TERM");
    // The second signal is best effort: the process may already have gone,
    // and `kill` then reports no such process. The point is that a second
    // one is survivable, not that it is delivered.
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(probe.pid().to_string())
        .status();

    let stopped = probe.finish();
    assert_eq!(
        stopped.status.code(),
        Some(0),
        "a second signal must not stop the process exiting cleanly; status was {:?}. \
         stderr was: {:?}",
        stopped.status,
        stopped.stderr
    );
    assert!(
        !stopped.stderr.contains("panicked"),
        "a second signal must not panic: {:?}",
        stopped.stderr
    );
    let summaries = stopped.stderr.matches(SUMMARY).count();
    assert_eq!(
        summaries, 1,
        "the guard must flush exactly once however many signals arrive, but stderr \
         held {summaries} summaries: {:?}",
        stopped.stderr
    );
}

/// The flush that already worked has to keep working: a client that closes
/// the stdio stream still gets the final summary, and the process still
/// exits 0.
#[test]
fn a_clean_eof_still_flushes_the_final_metrics_summary() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let mut probe = Probe::start_stdio(&cfg.path);
    probe.request(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    drop(probe.child.stdin.take());

    let stopped = probe.finish();
    assert_eq!(
        stopped.status.code(),
        Some(0),
        "a clean EOF must still exit 0; status was {:?}",
        stopped.status
    );
    assert!(
        stopped.stderr.contains(SUMMARY),
        "a clean EOF must still write the final metrics summary, but stderr was: {:?}",
        stopped.stderr
    );
}

/// AC: a server stopped by `SIGTERM` over the websocket transport flushes
/// its telemetry. command-mcp always compiles the websocket transport in
/// (the `mcp-core` dependency carries `features = ["auth"]`, which implies
/// it), so this is a transport the server really serves, not one gated
/// behind a Cargo feature this test file would need to match.
#[test]
fn sigterm_over_websocket_flushes_the_final_metrics_summary() {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let port = pick_unused_local_port();
    let mut probe = Probe::start_websocket(&cfg.path, port);
    probe.wait_until_listening();
    // The metrics summary is only written when it has something to report,
    // so a transport that served no request would leave the flush with
    // nothing to prove.
    websocket_initialize_request(port);

    probe.signal("TERM");
    let stopped = probe.finish();

    assert_eq!(
        stopped.status.code(),
        Some(0),
        "a signalled websocket server must exit 0; status was {:?}, stderr was {:?}",
        stopped.status,
        stopped.stderr
    );
    assert!(
        stopped.stderr.contains(SUMMARY),
        "a SIGTERM over websocket must still write the final metrics summary, \
         but stderr was: {:?}",
        stopped.stderr
    );
}

/// Connect to the websocket probe's `/ws` endpoint, send one `initialize`
/// request, and wait for its reply, so the metrics registry has something in
/// it before the probe is signalled. Blocks on a throwaway current-thread
/// runtime, mirroring how `Probe::request` proves readiness for stdio.
fn websocket_initialize_request(port: u16) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");
    runtime.block_on(async move {
        let url = format!("ws://127.0.0.1:{port}/ws");
        let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
            .await
            .expect("the probe must accept a websocket connection");
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
        });
        ws.send(Message::Text(request.to_string().into()))
            .await
            .expect("the probe must accept the request");
        let reply = ws
            .next()
            .await
            .expect("the probe must reply before closing the connection")
            .expect("a valid websocket message");
        let text = match reply {
            Message::Text(t) => t.to_string(),
            other => panic!("expected a text reply, got {other:?}"),
        };
        assert!(
            text.contains("\"result\""),
            "the probe must be serving before it is signalled, but replied {text:?}"
        );
    });
}

/// Start a stdio probe, drive one request through it so the metrics registry
/// has something in it, then stop it with `signal_name`.
fn stdio_probe_stopped_by(signal_name: &str) -> Stopped {
    let cfg = write_temp_config(&minimal_echo_config_toml());
    let mut probe = Probe::start_stdio(&cfg.path);
    probe.request(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    probe.signal(signal_name);
    let stopped = probe.finish();
    // Keep the temp config directory alive until the probe (which read it at
    // startup) has fully exited.
    drop(cfg);
    stopped
}

/// A running probe process, with its stderr being drained on another thread.
struct Probe {
    child: Child,
    stdout: BufReader<std::process::ChildStdout>,
    stderr: StderrTail,
}

/// What a stopped probe left behind.
struct Stopped {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl Probe {
    fn start_stdio(config_path: &Path) -> Self {
        let mut cmd = Command::new(command_mcp_bin());
        cmd.arg("serve")
            .arg("--config")
            .arg(config_path)
            .arg("--transport")
            .arg("stdio");
        Self::spawn(cmd)
    }

    fn start_websocket(config_path: &Path, port: u16) -> Self {
        let mut cmd = Command::new(command_mcp_bin());
        cmd.arg("serve")
            .arg("--config")
            .arg(config_path)
            .arg("--transport")
            .arg("websocket")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string());
        Self::spawn(cmd)
    }

    fn spawn(mut cmd: Command) -> Self {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("the probe must start");
        let stdout = BufReader::new(child.stdout.take().expect("the probe has a piped stdout"));
        let stderr = StderrTail::attach(child.stderr.take().expect("the probe has a piped stderr"));
        Self {
            child,
            stdout,
            stderr,
        }
    }

    /// Send one JSON-RPC request and read its reply, so the caller knows the
    /// server is up and has recorded a metric.
    fn request(&mut self, request: &str) {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .expect("the probe has a piped stdin");
        writeln!(stdin, "{request}").expect("the probe must accept its input");
        stdin.flush().expect("the request must reach the probe");
        let mut reply = String::new();
        self.stdout
            .read_line(&mut reply)
            .expect("the probe must answer the request");
        assert!(
            reply.contains("\"result\""),
            "the probe must be serving before it is signalled, but replied {reply:?}"
        );
    }

    /// Block until the probe reports that it is listening (websocket only;
    /// stdio has no such line and is proven ready by `request` instead).
    fn wait_until_listening(&mut self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let seen = self.stderr.snapshot();
            if seen.contains("listening") {
                return;
            }
            if let Some(status) = self
                .child
                .try_wait()
                .expect("the probe's state must be readable")
            {
                panic!(
                    "the probe stopped with {status:?} before it listened, so this test \
                     never ran. Its own report was: {seen:?}"
                );
            }
            assert!(
                Instant::now() < deadline,
                "the probe never reported listening within {READY_TIMEOUT:?}; \
                 what it did write was: {seen:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Send `signal_name` (as `kill` names it, so `TERM` or `INT`).
    fn signal(&self, signal_name: &str) {
        let status = Command::new("kill")
            .arg(format!("-{signal_name}"))
            .arg(self.pid().to_string())
            .status()
            .expect("kill must run, or this test proves nothing");
        assert!(
            status.success(),
            "kill -{signal_name} on the probe failed, so no signal was delivered"
        );
    }

    /// Wait for the probe to stop, then collect everything it wrote.
    fn finish(mut self) -> Stopped {
        let status = wait_for_exit(&mut self.child);
        let mut stdout = String::new();
        // The child has exited, so this reads to EOF without blocking.
        self.stdout.read_to_string(&mut stdout).unwrap_or_default();
        Stopped {
            status,
            stdout,
            stderr: self.stderr.finish(),
        }
    }
}

/// Wait for `child` to exit, and fail the test rather than hang if it does
/// not.
fn wait_for_exit(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match child
            .try_wait()
            .expect("the probe's state must be readable")
        {
            Some(status) => return status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                panic!(
                    "the probe did not stop within {STOP_TIMEOUT:?}, so the shutdown is \
                     not bounded"
                );
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Drains a child's stderr on its own thread.
///
/// Reading it only at the end would deadlock a server that fills the pipe
/// while the test is waiting on something else, and the websocket test has
/// to read a line from a process that is still running.
struct StderrTail {
    text: Arc<Mutex<String>>,
    reader: Option<JoinHandle<()>>,
}

impl StderrTail {
    fn attach(stderr: ChildStderr) -> Self {
        let text = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&text);
        let reader = std::thread::spawn(move || {
            let mut lines = BufReader::new(stderr).lines();
            while let Some(Ok(line)) = lines.next() {
                let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
                sink.push_str(&line);
                sink.push('\n');
            }
        });
        Self {
            text,
            reader: Some(reader),
        }
    }

    fn snapshot(&self) -> String {
        self.text.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Wait for stderr to reach EOF, then return everything it carried.
    fn finish(mut self) -> String {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.snapshot()
    }
}
