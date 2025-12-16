# Development Guide

Development setup and contribution guidelines for genmcp.

## Development Setup

### Prerequisites

- Rust 1.92 or later
- Cargo
- Git

### Getting Started

```bash
# Clone repository
git clone <repository-url>
cd genmcp

# Build
cargo build

# Run tests
cargo test

# Run clippy
cargo clippy -- -D warnings

# Format code
cargo fmt
```

## Project Structure

```
genmcp/
├── Cargo.toml          # Dependencies and build configuration
├── Dockerfile          # Docker build configuration
├── README.md           # Main documentation
├── PLAN.md             # Implementation plan
├── .cursorrules        # Development rules
├── src/
│   ├── main.rs         # CLI interface and entry point
│   ├── error.rs        # Error types
│   ├── config.rs       # Configuration parsing
│   ├── executor.rs     # Command execution
│   ├── tools.rs        # Tool registry
│   ├── server.rs       # MCP server
│   ├── transport.rs    # Transport layer
│   └── config_schema.rs # Schema generation
├── tests/
│   ├── common/         # Shared test utilities
│   ├── fixtures/       # Test data
│   ├── integration/    # Integration tests
│   └── unit/           # Unit tests
├── examples/
│   └── config.toml     # Example configuration
└── docs/               # Documentation
    ├── configuration.md
    ├── deployment.md
    ├── architecture.md
    └── development.md
```

## Development Workflow

### Work in Cohesive Chunks

- Complete a logical unit of work before committing
- Each chunk should be self-contained and functional
- Group related changes together (implementation + tests + documentation)

### After Each Change

1. Run `cargo test` - Fix any failing tests
2. Run `cargo build` - Must pass with no warnings
3. Run `cargo clippy -- -D warnings` - Must pass with no warnings
4. Resolve all errors and warnings before proceeding
5. Commit the cohesive chunk with a proper commit message

### Git Commit Strategy

Use conventional commit messages:

```
<type>(<scope>): <subject>

<body>
```

Types: `feat`, `fix`, `test`, `docs`, `refactor`, `chore`

Examples:
- `feat(config): add TOML parsing with nested groups structure`
- `test(executor): add unit tests for timeout and graceful termination`
- `fix(transport): handle malformed JSON-RPC messages correctly`

## Code Quality

### Warnings Policy

**All warnings are treated as errors**. The build must not produce any warnings.

- Configured in `Cargo.toml` with `[lints]` section
- Use `cargo clippy -- -D warnings` to check
- Only disable warnings with `#[allow(...)]` when there's a solid technical reason
- Always document the reason: `#[allow(warning_name)] // Reason: ...`

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use meaningful variable and function names
- Add comments for complex logic or non-obvious behavior
- Keep functions focused and reasonably sized

### Modularization

- **Proper design takes precedence over arbitrary limits**
- Aim to keep individual source files under 1000 lines when possible
- Split large modules into submodules when it improves organization
- Use Rust's module system to organize related functionality

## Testing

### Test Organization

- **Unit tests**: `#[cfg(test)] mod tests { ... }` in each source file
- **Integration tests**: `tests/` directory with separate test files
- **Test utilities**: `tests/common/mod.rs` for shared helpers
- **Fixtures**: `tests/fixtures/` for sample configs and test scripts

### Test Requirements

- Comprehensive coverage with edge cases
- Test both success and error paths
- Test boundary conditions and invalid inputs
- Tests define expected behavior - only modify tests if they are testing wrong behavior

### Running Tests

```bash
# All tests
cargo test

# Specific test module
cargo test config::tests

# With output
cargo test -- --nocapture

# Integration tests only
cargo test --test '*'
```

## Building

### Development Build

```bash
cargo build
```

### Release Build

```bash
cargo build --release
```

### Docker Build

```bash
docker build -t genmcp .
```

## Debugging

### Enable Debug Logging

Add logging to understand execution flow:

```rust
eprintln!("Debug: {:?}", value);
```

### Common Issues

1. **Compilation Errors**: Check that all dependencies are added to `Cargo.toml`
2. **Test Failures**: Run tests with `--nocapture` to see output
3. **Clippy Warnings**: Fix or document with `#[allow(...)]`
4. **Configuration Errors**: Validate TOML syntax and structure

## Contributing

### Before Submitting

1. Ensure all tests pass
2. Ensure `cargo clippy -- -D warnings` passes
3. Ensure `cargo build` passes with no warnings
4. Update documentation if needed
5. Write clear commit messages

### Pull Request Process

1. Create a feature branch
2. Make changes following development workflow
3. Ensure all checks pass
4. Submit pull request with clear description
5. Address review feedback

## Dependencies

### Adding Dependencies

```bash
# Search for crate
cargo search <crate-name>

# Add dependency
cargo add <crate-name>@<version>

# Add with features
cargo add <crate-name> --features <feature>
```

### Updating Dependencies

```bash
cargo update
```

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [MCP Specification](https://modelcontextprotocol.io/)
- [Tokio Documentation](https://tokio.rs/)

