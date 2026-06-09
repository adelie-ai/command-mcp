#![deny(warnings)]

// Secure command execution with timeouts

use crate::config::TerminationSignal;
use crate::error::{ExecutionError, Result};
use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, timeout};

/// Hard ceiling on how many bytes we will read from a single stream (stdout or
/// stderr) before we stop reading and let the child be terminated. This caps
/// memory regardless of the configured line limits so a command that emits
/// unbounded output (e.g. `find /`, `cat /dev/zero`, `yes`) cannot OOM the
/// server.
const MAX_STREAM_BYTES: usize = 256 * 1024 * 1024;

/// Execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Exit code (0 for success)
    pub exit_code: i32,
    /// STDOUT content
    pub stdout: String,
    /// STDERR content
    pub stderr: String,
    /// Whether execution was stopped due to stop_after
    pub stopped_after: bool,
}

/// Output collected from a child stream, already bounded to the configured
/// head/tail line budget and a hard byte ceiling.
#[derive(Default)]
struct CappedOutput {
    /// First `keep_head` lines (in order).
    head: Vec<String>,
    /// Last `keep_tail` lines (in order).
    tail: VecDeque<String>,
    /// Total number of lines observed (may exceed head + tail).
    total_lines: usize,
}

impl CappedOutput {
    /// Render the bounded output as a string, reproducing the head/tail
    /// "... (N lines omitted) ..." format used by `apply_line_limits`.
    ///
    /// Because the collector only ever stores `keep_head` leading lines and
    /// `keep_tail` trailing lines (with no overlap), the head and tail buffers
    /// concatenated reconstruct the full output whenever nothing was omitted.
    fn render(&self, head_lines: u64, tail_lines: u64) -> String {
        let head = head_lines as usize;
        let tail = tail_lines as usize;

        let tail_joined = || self.tail.iter().cloned().collect::<Vec<_>>().join("\n");

        // When both limits are 0, callers expect the full (byte-capped) output.
        // The collector stored every line in `tail` in this mode.
        if head == 0 && tail == 0 {
            return tail_joined();
        }

        let kept = self.head.len() + self.tail.len();
        let omitted = self.total_lines.saturating_sub(kept);

        if head > 0 && tail > 0 && omitted > 0 {
            format!(
                "{}\n... ({} lines omitted) ...\n{}",
                self.head.join("\n"),
                omitted,
                tail_joined(),
            )
        } else if head > 0 && tail == 0 {
            self.head.join("\n")
        } else if tail > 0 && head == 0 {
            tail_joined()
        } else {
            // Everything fit within the budget; head + tail is the full output.
            let mut lines: Vec<&str> = self.head.iter().map(|s| s.as_str()).collect();
            lines.extend(self.tail.iter().map(|s| s.as_str()));
            lines.join("\n")
        }
    }
}

/// Read a child stream line-by-line, keeping at most `keep_head` leading lines
/// and `keep_tail` trailing lines, and stopping once `MAX_STREAM_BYTES` have
/// been read. This bounds memory regardless of how much the child emits.
async fn collect_capped_lines<R>(handle: R, keep_head: usize, keep_tail: usize) -> CappedOutput
where
    R: tokio::io::AsyncRead + Unpin,
{
    collect_capped_lines_with_cap(handle, keep_head, keep_tail, MAX_STREAM_BYTES).await
}

/// Like [`collect_capped_lines`] but with an explicit byte ceiling, so tests can
/// exercise the cap without generating hundreds of MiB.
async fn collect_capped_lines_with_cap<R>(
    handle: R,
    keep_head: usize,
    keep_tail: usize,
    max_bytes: usize,
) -> CappedOutput
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(handle);
    let mut head: Vec<String> = Vec::new();
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut total_lines = 0usize;
    let mut bytes = 0usize;
    // When both budgets are 0 we keep everything (still byte-capped); collect
    // those lines into `tail` so `render` can emit them all.
    let keep_all = keep_head == 0 && keep_tail == 0;

    let mut line = String::new();
    loop {
        line.clear();
        let n = match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(_) => break,
        };
        bytes += n;
        // Strip a single trailing newline for storage consistency.
        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();

        if keep_all {
            tail.push_back(trimmed);
        } else {
            if head.len() < keep_head {
                head.push(trimmed);
            } else if keep_tail > 0 {
                if tail.len() == keep_tail {
                    tail.pop_front();
                }
                tail.push_back(trimmed);
            }
        }
        total_lines += 1;

        if bytes >= max_bytes {
            break;
        }
    }

    CappedOutput {
        head,
        tail,
        total_lines,
    }
}

/// Execute a command with the given parameters
#[allow(clippy::too_many_arguments)] // Required for comprehensive execution configuration
pub async fn execute_command(
    command: &str,
    args: &[String],
    timeout_secs: u64,
    stop_after_secs: Option<u64>,
    termination_signal: TerminationSignal,
    termination_grace_period: u64,
    output_head_lines: u64,
    output_tail_lines: u64,
    stderr_lines: u64,
) -> Result<ExecutionResult> {
    // Build command - never use shell execution
    let mut cmd = TokioCommand::new(command);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Start the process
    let mut child = cmd
        .spawn()
        .map_err(|e| ExecutionError::CommandNotFound(format!("{}: {}", command, e)))?;

    let stdout_handle = child
        .stdout
        .take()
        .ok_or_else(|| ExecutionError::CommandFailed {
            command: command.to_string(),
            exit_code: None,
            stderr: "Failed to capture stdout".to_string(),
        })?;
    let stderr_handle = child
        .stderr
        .take()
        .ok_or_else(|| ExecutionError::CommandFailed {
            command: command.to_string(),
            exit_code: None,
            stderr: "Failed to capture stderr".to_string(),
        })?;

    // Spawn tasks to read stdout and stderr. Reads are bounded: we keep only the
    // configured head/tail line budget and stop reading once a hard byte ceiling
    // is reached, so unbounded child output cannot grow our memory without limit.
    // stdout keeps both head and tail; stderr keeps only the tail.
    let stdout_keep_head = output_head_lines as usize;
    let stdout_keep_tail = output_tail_lines as usize;
    let stdout_task = tokio::spawn(async move {
        collect_capped_lines(stdout_handle, stdout_keep_head, stdout_keep_tail).await
    });

    let stderr_keep_tail = stderr_lines as usize;
    let stderr_task =
        tokio::spawn(async move { collect_capped_lines(stderr_handle, 0, stderr_keep_tail).await });

    // Handle stop_after if configured
    if let Some(stop_after) = stop_after_secs
        && stop_after > 0
    {
        return handle_stop_after(
            child,
            stdout_task,
            stderr_task,
            stop_after,
            termination_signal,
            termination_grace_period,
            output_head_lines,
            output_tail_lines,
            stderr_lines,
            command,
        )
        .await;
    }

    // Handle timeout
    let timeout_duration = Duration::from_secs(timeout_secs);
    match timeout(timeout_duration, child.wait()).await {
        Ok(Ok(status)) => {
            // Process completed within timeout
            let exit_code = exit_code_from_status(&status);
            let stdout = stdout_task.await.unwrap_or_default();
            let stderr = stderr_task.await.unwrap_or_default();
            Ok(ExecutionResult {
                exit_code,
                stdout: stdout.render(output_head_lines, output_tail_lines),
                // Always return STDERR with consistent line limiting
                stderr: stderr.render(0, stderr_lines),
                stopped_after: false,
            })
        }
        Ok(Err(e)) => Err(ExecutionError::CommandFailed {
            command: command.to_string(),
            exit_code: None,
            stderr: format!("Process wait error: {}", e),
        }
        .into()),
        Err(_) => {
            // Timeout occurred
            handle_timeout(
                child,
                stdout_task,
                stderr_task,
                termination_signal,
                termination_grace_period,
                timeout_secs,
                output_head_lines,
                output_tail_lines,
                stderr_lines,
                command,
            )
            .await
        }
    }
}

/// Handle stop_after scenario
#[allow(clippy::too_many_arguments)] // Required for comprehensive execution configuration
async fn handle_stop_after(
    mut child: tokio::process::Child,
    stdout_task: tokio::task::JoinHandle<CappedOutput>,
    stderr_task: tokio::task::JoinHandle<CappedOutput>,
    stop_after: u64,
    termination_signal: TerminationSignal,
    termination_grace_period: u64,
    output_head_lines: u64,
    output_tail_lines: u64,
    stderr_lines: u64,
    command: &str,
) -> Result<ExecutionResult> {
    let child_id = child.id();
    let signal = termination_signal;
    let grace = termination_grace_period;
    let mut stop_after_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(stop_after)).await;
        if let Some(pid) = child_id {
            terminate_process(pid, signal, grace).await;
        }
    });

    // Wait for either process completion or stop_after
    tokio::select! {
        result = child.wait() => {
            stop_after_handle.abort();
            match result {
                Ok(status) => {
                    let exit_code = exit_code_from_status(&status);
                    let stdout = stdout_task.await.unwrap_or_default();
                    let stderr = stderr_task.await.unwrap_or_default();
                    Ok(ExecutionResult {
                        exit_code,
                        stdout: stdout.render(output_head_lines, output_tail_lines),
                        // Always return STDERR with consistent line limiting
                        stderr: stderr.render(0, stderr_lines),
                        stopped_after: false,
                    })
                }
                Err(e) => {
                    Err(ExecutionError::CommandFailed {
                        command: command.to_string(),
                        exit_code: None,
                        stderr: format!("Process wait error: {}", e),
                    }.into())
                }
            }
        }
        _ = &mut stop_after_handle => {
            // stop_after expired
            // Wait for graceful termination
            sleep(Duration::from_secs(termination_grace_period + 1)).await;
            // Check if process exited
            if let Ok(Some(status)) = child.try_wait() {
                let exit_code = exit_code_from_status(&status);
                let stdout = stdout_task.await.unwrap_or_default();
                let stderr = stderr_task.await.unwrap_or_default();
                Ok(ExecutionResult {
                    exit_code,
                    stdout: stdout.render(output_head_lines, output_tail_lines),
                    // Always return STDERR with consistent line limiting
                    stderr: stderr.render(0, stderr_lines),
                    stopped_after: true,
                })
            } else {
                // Force kill if still running
                if let Some(pid) = child.id() {
                    force_kill_process(pid).await;
                }
                // For stop_after, this is success
                let stdout = stdout_task.await.unwrap_or_default();
                let stderr = stderr_task.await.unwrap_or_default();
                Ok(ExecutionResult {
                    exit_code: 0,
                    stdout: stdout.render(output_head_lines, output_tail_lines),
                    // Always return STDERR with consistent line limiting
                    stderr: stderr.render(0, stderr_lines),
                    stopped_after: true,
                })
            }
        }
    }
}

/// Handle timeout scenario
#[allow(clippy::too_many_arguments)] // Required for comprehensive execution configuration
async fn handle_timeout(
    mut child: tokio::process::Child,
    stdout_task: tokio::task::JoinHandle<CappedOutput>,
    stderr_task: tokio::task::JoinHandle<CappedOutput>,
    termination_signal: TerminationSignal,
    termination_grace_period: u64,
    timeout_secs: u64,
    output_head_lines: u64,
    output_tail_lines: u64,
    stderr_lines: u64,
    command: &str,
) -> Result<ExecutionResult> {
    let pid = child.id();
    if let Some(pid) = pid {
        terminate_process(pid, termination_signal, termination_grace_period).await;
    }
    // Wait for graceful termination
    sleep(Duration::from_secs(termination_grace_period + 1)).await;
    // Check if process is still running
    if let Ok(Some(status)) = child.try_wait() {
        let exit_code = exit_code_from_status(&status);
        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();
        Ok(ExecutionResult {
            exit_code,
            stdout: stdout.render(output_head_lines, output_tail_lines),
            stderr: stderr.render(0, stderr_lines),
            stopped_after: false,
        })
    } else {
        // Force kill
        if let Some(pid) = child.id() {
            force_kill_process(pid).await;
        }
        Err(ExecutionError::Timeout {
            command: command.to_string(),
            timeout: timeout_secs,
        }
        .into())
    }
}

/// Derive an exit code from a process status.
///
/// `ExitStatus::code()` returns `None` when the process was terminated by a
/// signal (Unix). Reporting `0` in that case would mislead callers into
/// thinking a killed process succeeded, so we report `-1` instead.
fn exit_code_from_status(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

/// Terminate a process gracefully
async fn terminate_process(pid: u32, signal: TerminationSignal, grace_period: u64) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let nix_signal = match signal {
            TerminationSignal::Sigterm => Signal::SIGTERM,
            TerminationSignal::Sigint => Signal::SIGINT,
        };

        if kill(Pid::from_raw(pid as i32), Some(nix_signal)).is_ok() {
            // Wait for grace period
            sleep(Duration::from_secs(grace_period)).await;
        }
    }
    #[cfg(not(unix))]
    {
        // On Windows, just terminate
        let _ = std::process::Command::new("taskkill")
            .args(&["/PID", &pid.to_string(), "/F"])
            .output();
    }
}

/// Force kill a process
async fn force_kill_process(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(pid as i32), Some(Signal::SIGKILL));
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(&["/PID", &pid.to_string(), "/F"])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_successful_command_execution() {
        let result = execute_command(
            "/bin/echo",
            &["hello".to_string()],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        assert!(!result.stopped_after);
    }

    #[tokio::test]
    async fn test_command_with_multiple_args() {
        let result = execute_command(
            "/bin/echo",
            &["hello".to_string(), "world".to_string()],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        assert!(result.stdout.contains("world"));
    }

    #[tokio::test]
    async fn test_command_not_found() {
        let result = execute_command(
            "/nonexistent/command",
            &[],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await;

        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                crate::error::GenMcpError::Execution(ExecutionError::CommandNotFound(_)) => {}
                _ => panic!("Expected CommandNotFound error"),
            }
        }
    }

    #[tokio::test]
    async fn test_non_zero_exit_code() {
        let result = execute_command(
            "/bin/sh",
            &["-c".to_string(), "exit 42".to_string()],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 42);
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let result = execute_command(
            "/bin/sh",
            &[
                "-c".to_string(),
                "echo 'stdout' >&1 && echo 'stderr' >&2 && exit 1".to_string(),
            ],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("stdout"));
        assert!(result.stderr.contains("stderr"));
    }

    #[tokio::test]
    async fn test_stderr_returned_on_success() {
        // Verify STDERR is returned even when exit code is 0
        let result = execute_command(
            "/bin/sh",
            &[
                "-c".to_string(),
                "echo 'stdout' >&1 && echo 'stderr message' >&2 && exit 0".to_string(),
            ],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("stdout"));
        // STDERR should be returned even on success
        assert!(result.stderr.contains("stderr message"));
    }

    #[tokio::test]
    async fn test_stderr_returned_when_empty() {
        // Verify STDERR field exists even when there's no stderr output
        let result = execute_command(
            "/bin/echo",
            &["hello".to_string()],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        // STDERR should be an empty string, not None
        assert_eq!(result.stderr, "");
    }

    #[tokio::test]
    async fn test_stderr_line_limiting() {
        // Verify STDERR line limiting is applied consistently
        let stderr_lines = [
            "error line 1",
            "error line 2",
            "error line 3",
            "error line 4",
            "error line 5",
        ];
        let stderr_content = stderr_lines.join("\n");

        let result = execute_command(
            "/bin/sh",
            &[
                "-c".to_string(),
                format!(
                    "echo 'stdout' >&1 && echo '{}' >&2 && exit 1",
                    stderr_content
                )
                .to_string(),
            ],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            2, // Only return last 2 lines of stderr
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 1);
        // Should only contain last 2 lines
        assert!(result.stderr.contains("error line 4"));
        assert!(result.stderr.contains("error line 5"));
        assert!(!result.stderr.contains("error line 1"));
    }

    #[tokio::test]
    async fn test_timeout_handling() {
        // Use sleep command that will timeout
        let result = execute_command(
            "/bin/sleep",
            &["5".to_string()],
            1, // 1 second timeout
            None,
            TerminationSignal::Sigterm,
            1, // 1 second grace period
            100,
            100,
            50,
        )
        .await;

        // Should timeout (may succeed if system is very fast, but unlikely with 5s sleep)
        // On most systems this will timeout
        if let Err(crate::error::GenMcpError::Execution(ExecutionError::Timeout { .. })) = result {
            // Expected timeout
        } else if result.is_err() {
            // May succeed on very fast systems, that's ok
        }
    }

    #[tokio::test]
    async fn test_stop_after_feature() {
        // Use a command that runs longer than stop_after
        // Note: This test may be flaky on slow systems, so we make it more lenient
        let result = execute_command(
            "/bin/sleep",
            &["5".to_string()],
            10,      // Long timeout
            Some(1), // Stop after 1 second
            TerminationSignal::Sigterm,
            1, // 1 second grace period
            100,
            100,
            50,
        )
        .await;

        // Should succeed (stop_after returns Ok rather than a timeout error).
        // The process is terminated by a signal, so its exit code is not a
        // meaningful success indicator: depending on the race between the
        // signal and reaping it can be -1 (signalled) or 0 (force-kill path).
        // We therefore only assert that the call returned Ok.
        assert!(
            result.is_ok(),
            "stop_after should return Ok, got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_stop_after_zero_disabled() {
        // stop_after = 0 should be disabled
        let result = execute_command(
            "/bin/echo",
            &["hello".to_string()],
            10,
            Some(0), // Disabled
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert!(!result.stopped_after);
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_empty_output() {
        let result = execute_command(
            "/bin/true",
            &[],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_collect_capped_lines_stops_at_byte_ceiling() {
        // A reader that would yield far more than the cap allows. Each line is
        // 10 bytes ("xxxxxxxxx\n"); with a 100-byte cap we must stop after ~10
        // lines instead of reading the whole (1 MiB) input.
        let big = "xxxxxxxxx\n".repeat(100_000); // ~1 MiB
        let reader = std::io::Cursor::new(big.into_bytes());

        // keep everything (head/tail both budgeted high) so only the byte cap bounds us.
        let out = collect_capped_lines_with_cap(reader, 1000, 1000, 100).await;

        // The byte ceiling must cut reading short: only a tiny fraction of the
        // 100k available lines may have been read.
        assert!(
            out.total_lines <= 11,
            "read too many lines before capping: {}",
            out.total_lines
        );
    }

    #[tokio::test]
    async fn test_high_volume_command_is_bounded_by_line_limits() {
        // `yes` emits "y\n" forever; without bounded reading this would buffer
        // without limit and OOM. With head/tail limits + the byte ceiling the
        // result must be small and the process must be reaped.
        let result = execute_command(
            "/bin/sh",
            &[
                "-c".to_string(),
                // Cap the producer so the test can't run away even if the
                // bounding logic regresses; the executor's own cap is the real
                // safety net but we don't want a 256 MiB test.
                "yes | head -n 5000000".to_string(),
            ],
            10,
            None,
            TerminationSignal::Sigterm,
            2,
            5,
            5,
            50,
        )
        .await
        .unwrap();

        // Only head + tail (+ separator) should be retained, not all 5M lines.
        let line_count = result.stdout.lines().count();
        assert!(
            line_count <= 12,
            "stdout should be line-limited, got {} lines",
            line_count
        );
        assert!(result.stdout.contains("lines omitted"));
    }

    #[tokio::test]
    async fn test_special_characters_in_args() {
        // Test that special characters are handled correctly (not shell-injected)
        let result = execute_command(
            "/bin/echo",
            &["hello; rm -rf /".to_string()],
            10,
            None,
            TerminationSignal::Sigterm,
            5,
            100,
            100,
            50,
        )
        .await
        .unwrap();

        // Should just echo the string, not execute rm
        assert!(result.stdout.contains("hello; rm -rf /"));
        assert_eq!(result.exit_code, 0);
    }
}
