# AGENTS.md

Guidance for coding agents working in this repository.

## Project overview

`limes` is a Rust workspace for a Log In Manager & Screenlock.

- `crates/limes-cli`: CLI binary named `limes`.
- `crates/limes-core`: security-sensitive backend logic for auth, PAM/session boundaries, locking, frontend orchestration, config, and events.
- `crates/limes-proto`: shared types/events for frontends and backend code.
- `examples/limes-frontend-native`: starter native/text frontend executable.

Keep authentication, PAM/session handling, lock state, and session launch logic in `limes-core`. Frontends should render UI, collect input, and call backend APIs instead of duplicating auth/session logic.

## Development commands

Before committing Rust changes, run:

```sh
cargo fmt --all
cargo test --workspace
```

Useful smoke test with the development auth backend:

```sh
export LIMES_AUTH_BACKEND=dev
export LIMES_DEV_PASSWORD=secret
export LIMES_SESSION_COMMAND="sh -c true"
cargo run -p limes-cli -- login --builtin
```

## Nix

This repository has a flake. Use the dev shell when available:

```sh
nix develop
```

Package outputs include:

- `.#limes`
- `.#frontend-native`

## Coding notes

- Prefer small, focused modules and explicit error messages via `LimesError`.
- Do not log passwords or other secrets. `AuthRequest` debug output redacts the password; preserve that behavior.
- Clear credential buffers after use where practical.
- Keep `limes-proto` lightweight and dependency-minimal.
- Treat the current lock display backend as a placeholder; do not imply it provides a real secure screen lock until an actual display/session-lock backend is implemented.
- Environment-provided command parsing is intentionally simple whitespace splitting; prefer CLI arguments for commands needing quoting.

## Git hygiene

- Keep commits focused and use conventional-style subjects such as `feat:`, `fix:`, `docs:`, or `chore:`.
- Do not commit build artifacts from `target/` or local direnv state.
