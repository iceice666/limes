# AGENTS.md

Guidance for coding agents working in this repository.

## Project overview

`limes` is a Rust workspace for a login manager and screenlock library.

- `crates/limes-common`: shared security-sensitive auth, config, events, frontend launching, and errors.
- `crates/limes-lock`: screenlock state and display/session-lock backends.
- `crates/limes-login`: login-manager PAM/session boundaries, session discovery, and session launch.
- `crates/limes-proto`: lightweight shared types/events for frontends and backend code.
- `examples/simple-lock`: minimal iced/layer-shell lock frontend example.

Keep authentication in `limes-common`, lock state/display locking in `limes-lock`, and PAM session launch/cleanup in `limes-login`. Frontends should render UI, collect input, and call backend APIs instead of duplicating auth/session logic. There is no bundled CLI/app launcher crate.

## Development commands

Before committing Rust changes, run:

```sh
cargo fmt --all
cargo test --workspace
```

Useful lock frontend smoke test with the real PAM backend. Configure `/etc/pam.d/limes` first and run under a Wayland compositor with `ext-session-lock-v1` support:

```sh
cargo run -p limes-simple-lock -- lock
```

## Nix

This repository has a flake. Use the dev shell when available:

```sh
nix develop
```

Package outputs include:

- `.#simple-lock`

## Coding notes

- Prefer small, focused modules and explicit error messages via `LimesError`.
- Do not log passwords or other secrets. `AuthRequest` debug output redacts the password; preserve that behavior.
- Clear credential buffers after use where practical.
- Keep `limes-proto` lightweight and dependency-minimal.
- `LockRuntime` uses `WaylandSessionLockBackend` by default; `NoopDisplayBackend` is only a placeholder/test implementation. Do not imply non-Wayland or no-op backends provide a real secure screen lock.
- Environment-provided command parsing is intentionally simple whitespace splitting; prefer API-provided command vectors for commands needing quoting.

## Git hygiene

- Keep commits focused and use conventional-style subjects such as `feat:`, `fix:`, `docs:`, or `chore:`.
- Do not commit build artifacts from `target/` or local direnv state.
