# Convenient development commands for limes.

set dotenv-load

# Crate-specific recipes.
mod limes_cli "crates/limes-cli/Justfile"

# List available recipes.
default:
    @just --list --list-submodules

# Format all Rust crates.
fmt:
    cargo fmt --all

# Run workspace tests.
test:
    cargo test --workspace

# Run cargo check for the whole workspace.
check:
    cargo check --workspace

# Format, check, and test everything.
ci: fmt check test

# Build the workspace.
build:
    cargo build --workspace

# Build all flake package outputs.
nix-build:
    nix build .#limes
