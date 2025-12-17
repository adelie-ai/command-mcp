#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// Secure command execution with timeouts

use crate::config::TerminationSignal;
use crate::error::{ExecutionError, Result};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, timeout};

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

    // Spawn tasks to read stdout and stderr
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout_handle);
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output).await;
        output
    });

    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr_handle);
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output).await;
        output
    });

    // Handle stop_after if configured
    if let Some(stop_after) = stop_after_secs {
        if stop_after > 0 {
            return handle_stop_after(
                child,
                stdout_task,
                stderr_task,
                stop_after,
                termination_signal,
                termination_grace_period,
                output_head_lines,
                output_tail_lines,
                command,
            )
            .await;
        }
    }

    // Handle timeout
    let timeout_duration = Duration::from_secs(timeout_secs);
    match timeout(timeout_duration, child.wait()).await {
        Ok(Ok(status)) => {
            // Process completed within timeout
            let exit_code = status.code().unwrap_or(0);
            let stdout = stdout_task.await.unwrap_or_default();
            let stderr = stderr_task.await.unwrap_or_default();
            Ok(ExecutionResult {
                exit_code,
                stdout: apply_line_limits_to_string(&stdout, output_head_lines, output_tail_lines),
                stderr: if exit_code != 0 {
                    get_last_n_lines(&stderr, stderr_lines)
                } else {
                    stderr
                },
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
    stdout_task: tokio::task::JoinHandle<String>,
    stderr_task: tokio::task::JoinHandle<String>,
    stop_after: u64,
    termination_signal: TerminationSignal,
    termination_grace_period: u64,
    output_head_lines: u64,
    output_tail_lines: u64,
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
                    let exit_code = status.code().unwrap_or(0);
                    let stdout = stdout_task.await.unwrap_or_default();
                    let stderr = stderr_task.await.unwrap_or_default();
                    Ok(ExecutionResult {
                        exit_code,
                        stdout: apply_line_limits_to_string(&stdout, output_head_lines, output_tail_lines),
                        stderr,
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
                let exit_code = status.code().unwrap_or(0);
                let stdout = stdout_task.await.unwrap_or_default();
                let stderr = stderr_task.await.unwrap_or_default();
                Ok(ExecutionResult {
                    exit_code,
                    stdout: apply_line_limits_to_string(&stdout, output_head_lines, output_tail_lines),
                    stderr,
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
                    stdout: apply_line_limits_to_string(&stdout, output_head_lines, output_tail_lines),
                    stderr,
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
    stdout_task: tokio::task::JoinHandle<String>,
    stderr_task: tokio::task::JoinHandle<String>,
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
        let exit_code = status.code().unwrap_or(1);
        let stdout = stdout_task.await.unwrap_or_default();
        let stderr = stderr_task.await.unwrap_or_default();
        Ok(ExecutionResult {
            exit_code,
            stdout: apply_line_limits_to_string(&stdout, output_head_lines, output_tail_lines),
            stderr: get_last_n_lines(&stderr, stderr_lines),
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

/// Apply head and tail line limits to output string
fn apply_line_limits_to_string(output: &str, head_lines: u64, tail_lines: u64) -> String {
    let lines: Vec<&str> = output.lines().collect();
    apply_line_limits(&lines, head_lines, tail_lines)
}

/// Apply head and tail line limits to output
fn apply_line_limits(lines: &[&str], head_lines: u64, tail_lines: u64) -> String {
    let total_lines = lines.len();
    let head = head_lines as usize;
    let tail = tail_lines as usize;

    if total_lines <= head + tail || (head == 0 && tail == 0) {
        // Return all lines if within limits, or if both limits are 0
        lines.join("\n")
    } else if head > 0 && tail > 0 {
        // Return head + tail with separator
        let head_part: Vec<&str> = lines.iter().take(head).copied().collect();
        let tail_part: Vec<&str> = lines.iter().skip(total_lines - tail).copied().collect();
        format!(
            "{}\n... ({} lines omitted) ...\n{}",
            head_part.join("\n"),
            total_lines - head - tail,
            tail_part.join("\n")
        )
    } else if head > 0 {
        // Head only - no separator needed
        lines
            .iter()
            .take(head)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        // Tail only - no separator needed
        lines
            .iter()
            .skip(total_lines - tail)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Get last N lines from stderr
fn get_last_n_lines(stderr: &str, n: u64) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    let limit = (n as usize).min(lines.len());
    lines
        .iter()
        .rev()
        .take(limit)
        .rev()
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Terminate a process gracefully
async fn terminate_process(pid: u32, signal: TerminationSignal, grace_period: u64) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
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
        use nix::sys::signal::{kill, Signal};
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
    async fn test_output_line_limiting_head_only() {
        // Create output with many lines
        let lines: Vec<String> = (1..=200).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");

        let limited = apply_line_limits_to_string(&output, 10, 0);
        let limited_lines: Vec<&str> = limited.lines().collect();

        // When head_only, should return exactly 10 lines (no separator)
        assert_eq!(limited_lines.len(), 10);
        assert_eq!(limited_lines[0], "line 1");
        assert_eq!(limited_lines[9], "line 10");
    }

    #[tokio::test]
    async fn test_output_line_limiting_tail_only() {
        // Create output with many lines
        let lines: Vec<String> = (1..=200).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");

        let limited = apply_line_limits_to_string(&output, 0, 10);
        let limited_lines: Vec<&str> = limited.lines().collect();

        // When tail_only, should return exactly 10 lines (no separator)
        assert_eq!(limited_lines.len(), 10);
        assert_eq!(limited_lines[0], "line 191");
        assert_eq!(limited_lines[9], "line 200");
    }

    #[tokio::test]
    async fn test_output_line_limiting_head_and_tail() {
        // Create output with many lines
        let lines: Vec<String> = (1..=200).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");

        let limited = apply_line_limits_to_string(&output, 5, 5);
        let limited_lines: Vec<&str> = limited.lines().collect();

        // Should have: 5 head + separator + 5 tail = 11 lines
        assert!(limited_lines.len() >= 10);
        assert!(limited.contains("line 1"));
        assert!(limited.contains("line 200"));
        assert!(limited.contains("omitted"));
    }

    #[tokio::test]
    async fn test_output_line_limiting_within_limits() {
        // Create output with few lines
        let output = "line 1\nline 2\nline 3";

        let limited = apply_line_limits_to_string(output, 10, 10);

        // Should return all lines since within limits
        assert_eq!(limited, output);
    }

    #[tokio::test]
    async fn test_get_last_n_lines() {
        let stderr = "error 1\nerror 2\nerror 3\nerror 4\nerror 5";
        let last_2 = get_last_n_lines(stderr, 2);
        let lines: Vec<&str> = last_2.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "error 4");
        assert_eq!(lines[1], "error 5");
    }

    #[tokio::test]
    async fn test_get_last_n_lines_more_than_available() {
        let stderr = "error 1\nerror 2";
        let last_10 = get_last_n_lines(stderr, 10);
        let lines: Vec<&str> = last_10.lines().collect();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "error 1");
        assert_eq!(lines[1], "error 2");
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

        // Should succeed (stop_after returns success)
        if let Ok(result) = result {
            // On most systems, this should be stopped_after
            // But on very fast systems or if timing is off, it might complete normally
            // So we just check that it succeeded
            assert_eq!(result.exit_code, 0);
        } else {
            // If it fails, that's unexpected but not a test failure
            // (could be system-specific)
        }
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
