# Security Audit — command-mcp

**Date:** 2026-03-31
**Scope:** Generic MCP server framework/generator

---

## Medium Severity

### 1. WebSocket Auth Bypass When Unconfigured (MEDIUM)

When no JWT secret or JWKS verifier is configured, authentication succeeds for any token.

**Recommendation:** Default to rejecting connections when auth is not configured. Require an explicit `--no-auth` flag to disable.

---

### 2. No Config File Size Limit (MEDIUM)

Config files are read without size limits.

**Recommendation:** Check file size before parsing (e.g. reject > 1 MiB).

---

## Positive Findings

- `Command::new()` used directly (no shell invocation)
- Tool parameters passed via structured arguments
- JWT validation when configured
