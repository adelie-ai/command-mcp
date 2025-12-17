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