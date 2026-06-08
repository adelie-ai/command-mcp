# Contributing to gen-mcp

Thank you for your interest in contributing to gen-mcp! This document provides guidelines and instructions for contributing.

## Development Setup

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd gen-mcp
   ```

2. **Build the project**
   ```bash
   cargo build
   ```

3. **Run tests**
   ```bash
   cargo test
   ```

4. **Run linter**
   ```bash
   cargo clippy -- -D warnings
   ```

5. **Format code**
   ```bash
   cargo fmt
   ```

## Development Workflow

### Work in Cohesive Chunks

- Complete a logical unit of work before committing (e.g., implement a module, add tests, fix bugs)
- Each chunk should be self-contained and functional
- Avoid partial implementations that break the build
- Group related changes together (implementation + tests + documentation)

### After Each Change

1. Run `cargo test` - Fix any failing tests (do not modify tests unless they are incorrect)
2. Run `cargo build` - Must pass with no warnings (warnings are treated as errors)
3. Run `cargo clippy -- -D warnings` - Must pass with no warnings (linter warnings are treated as errors)
4. Resolve all errors and warnings before proceeding to next change
5. Commit the cohesive chunk with a proper commit message

### Git Commit Strategy

- Commit after completing a cohesive chunk of work
- Use conventional commit messages with clear, descriptive messages
- Format: `<type>(<scope>): <subject>`
  - Types: `feat`, `fix`, `test`, `docs`, `refactor`, `chore`
  - Scope: module name or area (e.g., `config`, `executor`, `mcp-server`)
  - Subject: concise description of what changed
- Include body if needed for context or breaking changes
- Examples:
  - `feat(config): add JWT authentication configuration`
  - `test(executor): add unit tests for timeout and graceful termination`
  - `fix(transport): handle malformed JSON-RPC messages correctly`

## Code Quality Standards

### Warning Policy

- **All warnings are treated as errors** - the build must not produce any warnings
- Configure `Cargo.toml` to deny warnings at the crate level
- Use `cargo clippy -- -D warnings` to treat clippy warnings as errors
- Only disable warnings with `#[allow(...)]` or `#[allow(clippy::...)]` when there's a solid technical reason
- Always document the reason: `#[allow(warning_name)] // Reason: ...`

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use meaningful variable and function names
- Add comments for complex logic or non-obvious behavior
- Keep functions focused and reasonably sized

### Modularization

- **Proper design takes precedence over arbitrary limits**
- Aim to keep individual source files under 1000 lines when possible
- Split large modules into submodules when it improves organization and maintainability
- Use Rust's module system (`mod` declarations) to organize related functionality
- Create submodules for:
  - Distinct functional areas within a module
  - Complex types and their implementations
  - Separate concerns (e.g., parsing vs validation, different transport types)

## Testing Requirements

### Test Modification Policy

- Tests define expected behavior - only modify tests if they are testing the wrong behavior
- If implementation changes, update tests only if the new behavior is correct
- Prefer fixing implementation to match tests over changing tests

### Test Coverage

- Write comprehensive unit tests for all functionality
- Include edge cases in test coverage
- Test both success and error paths
- Test boundary conditions and invalid inputs

### Test Organization

- Unit tests: `#[cfg(test)] mod tests { ... }` in each source file
- Integration tests: `tests/` directory with separate test files
- Test utilities: `tests/common/mod.rs` for shared test helpers
- Mock/fixture data: `tests/fixtures/` for sample configs and test scripts

## Documentation

### Documentation Requirements

- **Keep documentation up to date** - Update docs when making changes that affect:
  - User-facing behavior
  - Configuration format
  - CLI interface
  - API or protocol behavior
  - Installation or deployment procedures

### Documentation Structure

- **README.md**: Main project documentation
  - Project overview and purpose
  - Quick start guide
  - Installation instructions
  - Basic usage examples
  - Links to detailed documentation in `docs/`
  
- **docs/ directory**: Detailed documentation
  - `configuration.md` - Complete configuration reference
  - `deployment.md` - Deployment guides (Docker, bare metal)
  - `architecture.md` - System architecture and design decisions
  - `development.md` - Development setup and contribution guide
  - Additional topic-specific documentation as needed

## Pull Request Process

1. **Create a feature branch**
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes**
   - Follow the development workflow above
   - Ensure all tests pass
   - Ensure code builds without warnings
   - Update documentation as needed

3. **Commit your changes**
   - Use conventional commit messages
   - Make focused, logical commits

4. **Push and create a Pull Request**
   - Push your branch to the repository
   - Create a pull request with a clear description
   - Reference any related issues

5. **Code Review**
   - Address any feedback from reviewers
   - Ensure CI checks pass

## Areas for Contribution

- Bug fixes
- New features (discuss in issues first)
- Documentation improvements
- Test coverage improvements
- Performance optimizations
- Code refactoring and cleanup

## Questions?

If you have questions about contributing, please open an issue or start a discussion.

Thank you for contributing to gen-mcp!

