# Convenient development commands for limes.

set dotenv-load := true

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

# Run the builtin text login smoke test with the dev auth backend.
login-dev password="secret":
    LIMES_AUTH_BACKEND=dev \
    LIMES_DEV_PASSWORD="{{password}}" \
    LIMES_SESSION_COMMAND="sh -c true" \
    cargo run -p limes-cli -- login --builtin

# Run the native lock UI with the dev auth backend.
lock-ui password="secret":
    LIMES_AUTH_BACKEND=dev \
    LIMES_DEV_PASSWORD="{{password}}" \
    cargo run -p limes-frontend-native -- lock

# Launch the native frontend through the CLI login frontend path.
login-native password="secret":
    cargo build -p limes-frontend-native
    LIMES_AUTH_BACKEND=dev \
    LIMES_DEV_PASSWORD="{{password}}" \
    LIMES_SESSION_COMMAND="sh -c true" \
    cargo run -p limes-cli -- login --frontend target/debug/limes-frontend-native -- login
