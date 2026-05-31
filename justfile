# Demo justfile for genmcp MCP tools

# Default recipe (runs if no recipe is provided)
default:
  @echo "justfile demo: try 'just greet Alice', 'just --list', 'just --show greet'"

greet name="world":
  @echo "hello {{name}}"

build:
  @cargo build

test:
  @cargo test

fmt:
  @cargo fmt --all

clippy:
  @cargo clippy

pwd:
  @pwd

env-demo:
  @echo "FOO=$FOO"

# --- Local verification ("local CI") ---
# Run locally instead of GitHub Actions. `install-hooks` wires `check` into a
# git pre-push hook so it runs automatically before every push.
# Reuses the existing `build` (cargo build) and `test` (cargo test) recipes.
check: fmt-check lint build test
fmt-check:
  cargo fmt --check
lint:
  cargo clippy --all-targets -- -D warnings
test-integration:
  cargo test -- --ignored
premerge:
  git fetch origin
  git rebase origin/main
  just check
install-hooks:
  git config core.hooksPath .githooks
  @echo "pre-push hook active — bypass once with: git push --no-verify"