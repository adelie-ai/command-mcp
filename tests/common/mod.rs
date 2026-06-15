// Shared test utilities and helpers

#![allow(dead_code)] // Reason: shared helpers are not necessarily used by every integration test crate.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub struct TempConfig {
    _dir: TempDir,
    pub path: PathBuf,
}

pub fn minimal_echo_config_toml() -> String {
    r#"
[groups.test]
default_timeout = 30

  [[groups.test.tools]]
  name = "echo"
  description = "Echo command"
  command = "/bin/echo"
  arg_order = ["text"]

    [groups.test.tools.parameters.text]
    description = "Text to echo"
    required = true
"#
    .trim()
    .to_string()
}

pub fn write_temp_config(toml: &str) -> TempConfig {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, toml).expect("write config.toml");
    TempConfig { _dir: dir, path }
}

pub fn command_mcp_bin() -> PathBuf {
    // Cargo exposes the binary path as `CARGO_BIN_EXE_<bin-name>`, preserving the
    // hyphen in `command-mcp`.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_command-mcp") {
        return PathBuf::from(p);
    }

    // Fall back to locating the binary next to the test executables.
    // This is more robust across environments where Cargo doesn't propagate
    // CARGO_BIN_EXE_* env vars into the test runtime.
    let exe = std::env::current_exe().expect("current_exe");
    let debug_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("test exe should be in target/.../deps/");
    let bin = debug_dir.join(format!("command-mcp{}", std::env::consts::EXE_SUFFIX));
    if bin.exists() {
        return bin;
    }

    panic!(
        "could not locate command-mcp binary. Tried CARGO_BIN_EXE_command-mcp and {}",
        bin.display()
    );
}

pub fn pick_unused_local_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

pub async fn wait_for_tcp_connect(host: &str, port: u16, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::net::TcpStream::connect((host, port)).await {
            Ok(stream) => {
                drop(stream);
                return;
            }
            Err(_) => {
                if tokio::time::Instant::now() >= deadline {
                    panic!(
                        "timed out waiting for {}:{} to accept TCP connections",
                        host, port
                    );
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

pub fn random_secret_hex_32_bytes() -> String {
    let mut bytes = [0u8; 32];
    let mut f = std::fs::File::open("/dev/urandom").expect("open /dev/urandom");
    use std::io::Read;
    f.read_exact(&mut bytes).expect("read /dev/urandom");

    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).expect("hex write");
    }
    s
}

pub fn spawn_command_mcp_websocket(
    config_path: &Path,
    host: &str,
    port: u16,
    jwt_secret: Option<&str>,
) -> ChildGuard {
    let mut cmd = Command::new(command_mcp_bin());
    cmd.arg("serve")
        .arg("--config")
        .arg(config_path)
        .arg("--mode")
        .arg("websocket")
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string());

    if let Some(secret) = jwt_secret {
        cmd.arg("--jwt-secret").arg(secret);
    }

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().expect("spawn command-mcp websocket");

    // Drain stderr so the process can't block if it logs a lot.
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(_line)) = lines.next_line().await {
                // Intentionally ignore.
            }
        });
    }

    ChildGuard { child }
}

pub fn spawn_command_mcp_stdio(config_path: &Path) -> ChildGuard {
    let mut cmd = Command::new(command_mcp_bin());
    cmd.arg("serve")
        .arg("--config")
        .arg(config_path)
        .arg("--mode")
        .arg("stdio")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    ChildGuard {
        child: cmd.spawn().expect("spawn command-mcp stdio"),
    }
}

pub struct ChildGuard {
    pub child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Best-effort; tests shouldn't hang if the child is still running.
        let _ = self.child.start_kill();
    }
}
