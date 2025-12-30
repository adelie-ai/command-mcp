# Configuration Reference

Complete reference for the genmcp configuration file format.

## Overview

The genmcp configuration file uses TOML format and organizes tools into functional groups. Groups provide default values that tools inherit, and tools can override these defaults. MAX values prevent LLMs from exceeding resource limits.

## File Structure

```toml
[groups.group_name]
# Group-level defaults
default_timeout = 30
default_timeout_max = 300
# ... more defaults ...

  [[groups.group_name.tools]]
  name = "tool_name"
  description = "Tool description"
  command = "/path/to/command"
  # Tool-level overrides (optional)
  
    [groups.group_name.tools.parameters.param_name]
    description = "Parameter description"
    example = "example_value"
    required = true
```

## Group Configuration

Each group can define default values that apply to all tools in that group:

### Timeout Settings

- `default_timeout` (optional, integer): Default timeout in seconds. Default: 30
- `default_timeout_max` (optional, integer): Maximum timeout that LLMs cannot exceed. Default: 300

### Stop After Settings

- `default_stop_after` (optional, integer): Default duration in seconds for long-running processes (0 = disabled). Default: 0
- `default_stop_after_max` (optional, integer): Maximum stop_after duration. Default: 3600

### Termination Settings

- `default_termination_signal` (optional, string): Default termination signal. Must be "SIGTERM" or "SIGINT". Default: "SIGTERM"
- `default_termination_grace_period` (optional, integer): Grace period in seconds to wait after sending termination signal before force-killing. Default: 5

### Output Limits

- `default_output_head_lines` (optional, integer): Default number of lines from head of output. Default: 100
- `default_output_tail_lines` (optional, integer): Default number of lines from tail of output. Default: 100
- `default_output_head_lines_max` (optional, integer): Maximum head lines. Default: 1000
- `default_output_tail_lines_max` (optional, integer): Maximum tail lines. Default: 1000

### STDERR Settings

- `default_stderr_lines` (optional, integer): Default number of STDERR lines to capture on error. Default: 50
- `default_stderr_lines_max` (optional, integer): Maximum STDERR lines. Default: 500

## Tool Configuration

Each tool must specify:

- `name` (required, string): Base tool name. Final tool name will be `{group_name}_{tool_name}`
- `description` (required, string): Description for the LLM
- `command` (required, string): Command to execute (absolute path recommended)

Optional tool-level overrides (same fields as group defaults):

- `timeout`, `timeout_max`: Override group timeout settings
- `stop_after`, `stop_after_max`: Override group stop_after settings
- `termination_signal`: Override group termination signal
- `termination_grace_period`: Override group grace period
- `output_head_lines`, `output_head_lines_max`: Override output limits
- `output_tail_lines`, `output_tail_lines_max`: Override output limits
- `stderr_lines`, `stderr_lines_max`: Override STDERR limits

## Parameter Definitions

Each tool can define parameters that the LLM can provide:

```toml
[groups.group_name.tools.parameters.param_name]
description = "Parameter description for LLM"
example = "example_value"  # Optional
required = true  # Optional, default: false
```

- `description` (required, string): Description of the parameter
- `example` (optional, string): Example value to help LLM understand usage
- `required` (optional, boolean): Whether parameter is required. Default: false

## Default Inheritance

1. Tool inherits from group defaults
2. Tool can override group defaults
3. LLM can override at runtime (within MAX constraints)

## MAX Value Constraints

MAX values are hard limits that LLMs cannot exceed. If an LLM tries to override a value beyond the MAX, the request will be rejected with an error.

## WebSocket Authentication Configuration

For WebSocket mode, you can configure JWT authentication:

```toml
[websocket_auth]
enabled = true  # Enable JWT authentication (default: true if section exists)
secret = "your-secret-key-here"  # Required if enabled=true
```

- `enabled` (optional, boolean): Enable JWT authentication. Default: `true` if `[websocket_auth]` section exists
- `secret` (optional, string): JWT secret key for token validation. Required if `enabled = true`

**To disable authentication entirely**, omit the `[websocket_auth]` section from your configuration file.

**CLI Override**: The `--jwt-secret` CLI option takes precedence over the config file setting.

## Validation Rules

- `timeout_max` must be >= `timeout` (if both specified)
- `stop_after_max` must be >= `stop_after` (if both specified)
- `output_head_lines_max` must be >= `output_head_lines` (if both specified)
- `output_tail_lines_max` must be >= `output_tail_lines` (if both specified)
- `stderr_lines_max` must be >= `stderr_lines` (if both specified)
- `termination_signal` must be "SIGTERM" or "SIGINT"
- Tool names must be unique across all groups (final name: `{group}_{tool}`)
- If `websocket_auth.enabled = true`, then `websocket_auth.secret` is required

## Examples

See `examples/config.toml` for comprehensive examples with common Unix commands.
See `examples/docker_config.toml` for Docker command examples.
See `examples/aws_cli_config.toml` for AWS CLI (aws) command examples.

