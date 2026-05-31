#![deny(warnings)]
#![allow(dead_code)] // Types will be used as implementation progresses

// STDIN/STDOUT and WebSocket transport handlers

use crate::error::{Result, TransportError};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioFraming {
    /// Detect framing based on the first successfully read message.
    Auto,
    /// Newline-delimited JSON messages (legacy/simple mode).
    Newline,
    /// JSON-RPC/LSP style framing: `Content-Length: N\r\n\r\n<json bytes>`
    ContentLength,
}

fn trim_crlf(s: &str) -> &str {
    s.trim_end_matches(&['\r', '\n'][..])
}

fn parse_content_length_header(line: &str) -> Option<usize> {
    // Be liberal in what we accept: case-insensitive header name, optional whitespace.
    let line = trim_crlf(line).trim();
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    let value = value.trim();
    value.parse::<usize>().ok()
}

/// STDIN/STDOUT transport for MCP
pub struct StdioTransportHandler {
    stdin: BufReader<Stdin>,
    stdout: Stdout,
    framing: StdioFraming,
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
            framing: StdioFraming::Auto,
        }
    }

    /// Read a JSON-RPC message from stdin.
    ///
    /// Supports both:
    /// - Newline-delimited JSON (legacy)
    /// - `Content-Length: N\r\n\r\n...` framing (common JSON-RPC/LSP-style framing)
    pub async fn read_message(&mut self) -> Result<String> {
        match self.framing {
            StdioFraming::Auto => self.read_message_auto().await,
            StdioFraming::Newline => self.read_message_newline().await,
            StdioFraming::ContentLength => self.read_message_content_length().await,
        }
    }

    /// Write a JSON-RPC message to stdout, using the detected framing mode.
    pub async fn write_message(&mut self, message: &str) -> Result<()> {
        match self.framing {
            StdioFraming::ContentLength => self.write_message_content_length(message).await,
            StdioFraming::Auto | StdioFraming::Newline => self.write_message_newline(message).await,
        }
    }

    async fn write_message_newline(&mut self, message: &str) -> Result<()> {
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

    async fn write_message_content_length(&mut self, message: &str) -> Result<()> {
        let bytes = message.as_bytes();
        // Use CRLF per JSON-RPC/LSP convention.
        let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
        self.stdout
            .write_all(header.as_bytes())
            .await
            .map_err(TransportError::Io)?;
        self.stdout
            .write_all(bytes)
            .await
            .map_err(TransportError::Io)?;
        self.stdout.flush().await.map_err(TransportError::Io)?;
        Ok(())
    }

    async fn read_message_newline(&mut self) -> Result<String> {
        let mut line = String::new();
        let n = self
            .stdin
            .read_line(&mut line)
            .await
            .map_err(TransportError::Io)?;
        if n == 0 {
            return Err(TransportError::ConnectionClosed.into());
        }
        Ok(trim_crlf(&line).to_string())
    }

    async fn read_message_auto(&mut self) -> Result<String> {
        // Skip leading empty lines (best-effort).
        loop {
            let mut line = String::new();
            let n = self
                .stdin
                .read_line(&mut line)
                .await
                .map_err(TransportError::Io)?;
            if n == 0 {
                return Err(TransportError::ConnectionClosed.into());
            }

            let line_trimmed = trim_crlf(&line);
            if line_trimmed.trim().is_empty() {
                continue;
            }

            if parse_content_length_header(line_trimmed).is_some() {
                // We already consumed the Content-Length header line; stash it and
                // proceed as content-length framed.
                self.framing = StdioFraming::ContentLength;
                return self
                    .read_message_content_length_with_first_line(line_trimmed)
                    .await;
            }

            // Treat as newline-delimited JSON.
            self.framing = StdioFraming::Newline;
            return Ok(line_trimmed.to_string());
        }
    }

    async fn read_message_content_length(&mut self) -> Result<String> {
        // Read the first header line (must include Content-Length).
        let mut first = String::new();
        let n = self
            .stdin
            .read_line(&mut first)
            .await
            .map_err(TransportError::Io)?;
        if n == 0 {
            return Err(TransportError::ConnectionClosed.into());
        }
        let first = trim_crlf(&first);
        self.read_message_content_length_with_first_line(first)
            .await
    }

    async fn read_message_content_length_with_first_line(&mut self, first: &str) -> Result<String> {
        let content_length = parse_content_length_header(first).ok_or_else(|| {
            TransportError::InvalidMessage(format!(
                "Expected Content-Length header, got: {}",
                first
            ))
        })?;

        // Read headers until blank line.
        loop {
            let mut header_line = String::new();
            let n = self
                .stdin
                .read_line(&mut header_line)
                .await
                .map_err(TransportError::Io)?;
            if n == 0 {
                return Err(TransportError::ConnectionClosed.into());
            }
            let header_line = trim_crlf(&header_line);
            if header_line.is_empty() {
                break;
            }
        }

        // Read exactly `content_length` bytes for the JSON payload.
        let mut buf = vec![0u8; content_length];
        self.stdin
            .read_exact(&mut buf)
            .await
            .map_err(TransportError::Io)?;

        let s = String::from_utf8(buf).map_err(|e| {
            TransportError::InvalidMessage(format!("Invalid UTF-8 in JSON-RPC message: {}", e))
        })?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_length_header() {
        assert_eq!(
            parse_content_length_header("Content-Length: 10\r\n"),
            Some(10)
        );
        assert_eq!(parse_content_length_header("content-length: 0\n"), Some(0));
        assert_eq!(
            parse_content_length_header("Content-Length:   42"),
            Some(42)
        );
        assert_eq!(
            parse_content_length_header("Content-Type: application/json"),
            None
        );
        assert_eq!(parse_content_length_header("nope"), None);
    }

    #[test]
    fn test_stdio_transport_handler_creation() {
        let handler = StdioTransportHandler::new();
        // Just verify it can be created
        let _ = handler;
    }
}
