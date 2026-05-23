# Convenient development commands for limes.

set dotenv-load

# List available recipes.
default:
    @just --list

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

# Run the simple lock frontend example.
simple-lock:
    cargo run -p limes-simple-lock -- lock

# Run the full-screen session-lock frontend example.
full-screenlock:
    cargo run -p limes-full-screenlock -- lock

# Preview the full-screen lock UI without locking the session.
full-screenlock-preview:
    cargo run -p limes-full-screenlock -- preview

# Build all flake package outputs.
nix-build:
    nix build .#simple-lock
