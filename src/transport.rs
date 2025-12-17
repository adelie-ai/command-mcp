#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// STDIN/STDOUT and WebSocket transport handlers

use crate::error::{Result, TransportError};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};

/// STDIN/STDOUT transport for MCP
pub struct StdioTransportHandler {
    stdin: BufReader<Stdin>,
    stdout: Stdout,
}

impl Default for StdioTransportHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioTransportHandler {
    /// Create a new STDIN/STDOUT transport handler
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(io::stdin()),
            stdout: io::stdout(),
        }
    }

    /// Read a JSON-RPC message from stdin (newline-delimited)
    pub async fn read_message(&mut self) -> Result<String> {
        let mut line = String::new();
        self.stdin
            .read_line(&mut line)
            .await
            .map_err(TransportError::Io)?;
        Ok(line.trim().to_string())
    }

    /// Write a JSON-RPC message to stdout (newline-delimited)
    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        self.stdout
            .write_all(message.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        self.stdout
            .write_all(b"\n")
            .await
            .map_err(TransportError::Io)?;
        self.stdout.flush().await.map_err(TransportError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_transport_handler_creation() {
        let handler = StdioTransportHandler::new();
        // Just verify it can be created
        let _ = handler;
    }
}
