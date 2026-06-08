# VS Code MCP Configuration Examples

This document shows how to configure gen-mcp as an MCP server in VS Code.

## Configuration Location

VS Code MCP server configurations are typically stored in:
- **macOS/Linux**: `~/.config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`
- **Windows**: `%APPDATA%\Code\User\globalStorage\saoudrizwan.claude-dev\settings\cline_mcp_settings.json`

Or in your VS Code settings.json:
```json
{
  "mcpServers": {
    // Your MCP server configurations here
  }
}
```

## Basic Configuration (STDIN/STDOUT Mode)

### Using the Example Config File

```json
{
  "mcpServers": {
    "gen-mcp-file-ops": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/examples/unixtools_config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

### Using the Docker Config

```json
{
  "mcpServers": {
    "gen-mcp-docker": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/examples/docker_config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

### Using a Custom Config File

```json
{
  "mcpServers": {
    "gen-mcp-custom": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/home/user/my-tools.toml",
        "--mode",
        "stdio"
      ],
      "env": {
        "PATH": "/usr/local/bin:/usr/bin:/bin"
      }
    }
  }
}
```

## WebSocket Mode Configuration

If you want to run gen-mcp as a WebSocket server (useful for remote access or multiple clients):

```json
{
  "mcpServers": {
    "gen-mcp-websocket": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/config.toml",
        "--mode",
        "websocket",
        "--host",
        "localhost",
        "--port",
        "8080"
      ],
      "env": {}
    }
  }
}
```

**Note**: WebSocket mode requires authentication. You'll need to provide a Bearer token in the connection. For now, any non-empty token is accepted (stub implementation).

## Using Docker Image

If you've built a Docker image of gen-mcp:

```json
{
  "mcpServers": {
    "gen-mcp-docker": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-v",
        "/path/to/config.toml:/config.toml:ro",
        "gen-mcp:latest",
        "serve",
        "--config",
        "/config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

## Multiple Server Configurations

You can configure multiple gen-mcp instances with different config files:

```json
{
  "mcpServers": {
    "gen-mcp-file-ops": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/file_ops_config.toml",
        "--mode",
        "stdio"
      ]
    },
    "gen-mcp-docker": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/docker_config.toml",
        "--mode",
        "stdio"
      ]
    },
    "gen-mcp-system": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/system_tools_config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

## Environment Variables

You can set environment variables for the gen-mcp process:

```json
{
  "mcpServers": {
    "gen-mcp-with-env": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/path/to/config.toml",
        "--mode",
        "stdio"
      ],
      "env": {
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "DOCKER_HOST": "unix:///var/run/docker.sock",
        "HOME": "/home/user"
      }
    }
  }
}
```

## Using Absolute Paths

Always use absolute paths for the config file:

```json
{
  "mcpServers": {
    "gen-mcp": {
      "command": "/usr/local/bin/gen-mcp",
      "args": [
        "serve",
        "--config",
        "/home/user/.config/gen-mcp/config.toml",
        "--mode",
        "stdio"
      ]
    }
  }
}
```

## Troubleshooting

### Server Not Starting

1. **Check the command path**: Make sure `gen-mcp` is in your PATH or use an absolute path
2. **Check config file path**: Use absolute paths, not relative paths
3. **Check file permissions**: Ensure the config file is readable
4. **Check logs**: VS Code should show MCP server logs in the output panel

### Tools Not Appearing

1. **Verify config file**: Use `gen-mcp config schema` to view the generated JSON Schema
2. **Check tool names**: Tool names are prefixed with group name (e.g., `docker_run`, `file_operations_mv`)
3. **Restart VS Code**: After changing MCP configuration, restart VS Code

### Permission Errors

If tools require elevated permissions:
- Ensure the gen-mcp process has necessary permissions
- Consider using `sudo` (not recommended for security reasons)
- Better: Configure tools to use paths that don't require elevated permissions

## Example: Complete Setup

Here's a complete example with multiple gen-mcp servers:

```json
{
  "mcpServers": {
    "gen-mcp-file-operations": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/home/user/.config/gen-mcp/file_ops.toml",
        "--mode",
        "stdio"
      ],
      "env": {
        "PATH": "/usr/local/bin:/usr/bin:/bin"
      }
    },
    "gen-mcp-docker": {
      "command": "gen-mcp",
      "args": [
        "serve",
        "--config",
        "/home/user/.config/gen-mcp/docker.toml",
        "--mode",
        "stdio"
      ],
      "env": {
        "PATH": "/usr/local/bin:/usr/bin:/bin",
        "DOCKER_HOST": "unix:///var/run/docker.sock"
      }
    }
  }
}
```

## Testing the Configuration

After adding the configuration:

1. Restart VS Code
2. Open the MCP panel (if available in your VS Code extension)
3. Check that the server shows as "connected"
4. Try using a tool from your configuration

## Schema Generation

To see what tools are available, you can generate the schema:

```bash
gen-mcp config schema > schema.json
```

This will show you all the tools, their parameters, and constraints defined in your config file.

