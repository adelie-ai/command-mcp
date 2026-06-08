#![deny(warnings)]

// Binary crate for gen-mcp — wires gen-mcp's dynamic, config-driven tools onto
// the shared mcp-core protocol/transport/CLI. The JSON-RPC dispatch, framing,
// transports, and websocket Bearer-token auth all come from mcp-core; this
// binary owns only the CLI (which keeps gen-mcp's `config` helper subcommands
// and its `--config`/`--jwt-secret`/`--oidc-issuer` flags) and the mapping from
// the TOML `[websocket_auth]` config to mcp-core's `WsAuth`.

use clap::{Args, Parser, Subcommand};
use gen_mcp::config::Config;
use gen_mcp::error::Result;
use gen_mcp::service::GenMcpService;
use mcp_core::{CommonServeArgs, ServerConfig, ServerCore, WsAuth};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "gen-mcp")]
#[command(about = "Generic MCP Script Adapter Server")]
#[command(
    long_about = "gen-mcp turns existing command-line programs (scripts, binaries, and CLIs) into an MCP server.\n\nPrimary workflow:\n  1) Generate a starting config: gen-mcp config example > config.toml\n  2) Edit config.toml to define your tools\n  3) Run in stdio mode (VS Code): gen-mcp serve --config config.toml --transport stdio\n  4) Or run in websocket mode (hosted): gen-mcp serve --config config.toml --transport websocket --host 0.0.0.0 --port 8080\n\nTip: Use `gen-mcp config schema > schema.json` to view the exact config structure."
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the MCP server
    Serve {
        #[command(flatten)]
        local: ServeArgs,
        /// Common transport flags (`--transport`/`--mode`, `--host`, `--port`,
        /// `--socket-path`) provided by mcp-core.
        #[command(flatten)]
        common: CommonServeArgs,
    },
    /// Configuration helpers (schema/docs/examples)
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

/// gen-mcp-specific `serve` flags, flattened alongside mcp-core's
/// [`CommonServeArgs`].
#[derive(Args)]
struct ServeArgs {
    /// Path to TOML configuration file
    #[arg(short, long, env = "GENMCP_CONFIG")]
    config: String,
    /// JWT secret for WebSocket authentication (legacy, optional). Overrides the
    /// config file's `[websocket_auth]` secret.
    #[arg(long)]
    jwt_secret: Option<String>,
    /// OIDC issuer URL for JWT validation via JWKS (preferred over jwt-secret).
    /// Overrides the config file's `[websocket_auth]`.
    #[arg(long)]
    oidc_issuer: Option<String>,
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Output generated JSON Schema for the TOML configuration structure
    Schema,
    /// Output an example TOML configuration file
    Example,
    /// Output Markdown documentation for the configuration file format
    Docs {
        /// If set, output the curated (hand-written) docs instead of generated docs.
        ///
        /// By default, docs are generated from the Rust config structures so they stay in sync.
        #[arg(long)]
        curated: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { local, common } => {
            // Load the gen-mcp TOML tool config (this also validates the
            // `[websocket_auth]` section, e.g. mutually-exclusive methods).
            let config = Config::from_file(&local.config)?;

            // Map the config's websocket auth (with CLI overrides) to mcp-core's
            // WsAuth before moving the config into the service.
            let ws_auth = resolve_ws_auth(
                config.websocket_auth.as_ref(),
                local.jwt_secret,
                local.oidc_issuer,
            );

            // Build the dynamic, config-driven service.
            let service = GenMcpService::new(config)?;

            // Hand mcp-core the config + service and serve over the selected
            // transport. The default transport set (stdio + websocket) already
            // permits websocket because we built with the `auth` feature (which
            // implies `websocket`); auth is only enforced when ws_auth != None.
            let server_config = ServerConfig::new("gen-mcp", env!("CARGO_PKG_VERSION"))
                .tools_list_changed(false)
                .websocket_auth(ws_auth);
            let core = ServerCore::new(server_config, Arc::new(service));
            mcp_core::serve(core, &common).await?;
        }
        Commands::Config { command } => match command {
            ConfigCommands::Schema => gen_mcp::config_schema::output_generated_schema()?,
            ConfigCommands::Example => gen_mcp::config_schema::output_generated_example_config()?,
            ConfigCommands::Docs { curated } => {
                if curated {
                    gen_mcp::config_schema::output_docs_curated()?
                } else {
                    gen_mcp::config_schema::output_docs_generated()?
                }
            }
        },
    }

    Ok(())
}

/// Map gen-mcp's TOML `[websocket_auth]` (plus the `--jwt-secret`/`--oidc-issuer`
/// CLI overrides) onto mcp-core's [`WsAuth`].
///
/// Precedence mirrors the historical behaviour: an `--oidc-issuer` override wins
/// over `--jwt-secret`, and both override the config file. The config file's
/// validation already rejects specifying both a secret and an OIDC/JWKS method,
/// so the config branch is unambiguous.
fn resolve_ws_auth(
    config_auth: Option<&gen_mcp::config::WebSocketAuth>,
    jwt_secret_override: Option<String>,
    oidc_issuer_override: Option<String>,
) -> WsAuth {
    // CLI overrides take precedence (OIDC over secret), matching the old server.
    if let Some(issuer) = oidc_issuer_override {
        return WsAuth::OidcIssuer(issuer);
    }
    if let Some(secret) = jwt_secret_override {
        return WsAuth::Secret(secret);
    }

    // Otherwise derive from the config file's [websocket_auth] section.
    match config_auth {
        // Disabled or absent → no auth.
        None => WsAuth::None,
        Some(auth) if !auth.enabled => WsAuth::None,
        Some(auth) => {
            // Validation guarantees exactly one method is set when enabled.
            if let Some(issuer) = &auth.oidc_issuer {
                WsAuth::OidcIssuer(issuer.clone())
            } else if let Some(jwks_url) = &auth.jwks_url {
                WsAuth::Jwks(jwks_url.clone())
            } else if let Some(secret) = &auth.secret {
                WsAuth::Secret(secret.clone())
            } else {
                // Unreachable in practice (validation requires a method when
                // enabled), but fail closed rather than silently disabling auth.
                WsAuth::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_auth_none_when_config_absent() {
        assert!(matches!(resolve_ws_auth(None, None, None), WsAuth::None));
    }

    #[test]
    fn ws_auth_none_when_disabled() {
        let auth = gen_mcp::config::WebSocketAuth {
            enabled: false,
            secret: Some("s".into()),
            oidc_issuer: None,
            jwks_url: None,
        };
        assert!(matches!(
            resolve_ws_auth(Some(&auth), None, None),
            WsAuth::None
        ));
    }

    #[test]
    fn ws_auth_secret_from_config() {
        let auth = gen_mcp::config::WebSocketAuth {
            enabled: true,
            secret: Some("topsecret".into()),
            oidc_issuer: None,
            jwks_url: None,
        };
        match resolve_ws_auth(Some(&auth), None, None) {
            WsAuth::Secret(s) => assert_eq!(s, "topsecret"),
            other => panic!("expected Secret, got {other:?}"),
        }
    }

    #[test]
    fn ws_auth_oidc_from_config() {
        let auth = gen_mcp::config::WebSocketAuth {
            enabled: true,
            secret: None,
            oidc_issuer: Some("https://issuer.example".into()),
            jwks_url: None,
        };
        match resolve_ws_auth(Some(&auth), None, None) {
            WsAuth::OidcIssuer(u) => assert_eq!(u, "https://issuer.example"),
            other => panic!("expected OidcIssuer, got {other:?}"),
        }
    }

    #[test]
    fn ws_auth_jwks_from_config() {
        let auth = gen_mcp::config::WebSocketAuth {
            enabled: true,
            secret: None,
            oidc_issuer: None,
            jwks_url: Some("https://issuer.example/jwks.json".into()),
        };
        match resolve_ws_auth(Some(&auth), None, None) {
            WsAuth::Jwks(u) => assert_eq!(u, "https://issuer.example/jwks.json"),
            other => panic!("expected Jwks, got {other:?}"),
        }
    }

    #[test]
    fn cli_secret_override_wins_over_config() {
        let auth = gen_mcp::config::WebSocketAuth {
            enabled: true,
            secret: None,
            oidc_issuer: Some("https://issuer.example".into()),
            jwks_url: None,
        };
        match resolve_ws_auth(Some(&auth), Some("override".into()), None) {
            WsAuth::Secret(s) => assert_eq!(s, "override"),
            other => panic!("expected Secret override, got {other:?}"),
        }
    }

    #[test]
    fn cli_oidc_override_wins_over_secret() {
        match resolve_ws_auth(
            None,
            Some("s".into()),
            Some("https://issuer.example".into()),
        ) {
            WsAuth::OidcIssuer(u) => assert_eq!(u, "https://issuer.example"),
            other => panic!("expected OidcIssuer override, got {other:?}"),
        }
    }
}
