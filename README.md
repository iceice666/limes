# limes

Login manager and screenlock library for Rust frontends.

`limes-core` is the project: it owns authentication, PAM/session boundaries,
Wayland session locking, session launch, config, and backend events. UI code is
expected to live in frontend applications that render, collect input, and call
`limes-core` APIs directly.

There is no bundled CLI. Applications and examples link to `limes-core` instead
of shelling out to a `limes` command.

## Architecture

- `crates/limes-core`: login manager and screenlock library. Contains the
  security-sensitive auth, PAM/session, lock, session launch, config, and event
  orchestration.
- `crates/limes-proto`: lightweight shared types/events for frontends and
  backend/library code.
- `examples/simple-lock`: minimal iced/layer-shell lock frontend that uses
  `limes-core` for Wayland session locking and PAM unlock authentication.

## Frontend integration

A login frontend should:

1. Build a `Runtime` from environment/config with `Runtime::from_env()`.
2. Collect username/password or PAM responses in the frontend UI.
3. Create an `AuthRequest` with `username`, `password`, and optional `tty`.
4. Call `runtime.authenticate(&request)`, then clear the secret with
   `request.clear_secret()`.
5. On success, call `runtime.start_session_for(&success)` or
   `runtime.start_session_for_with_command(&success, command)`, then
   `runtime.wait_session(&handle)` so `limes-core` handles PAM session
   open/close and user context switching.

A lock frontend should:

1. Build a `Runtime` with `Runtime::from_env()`.
2. Call `runtime.lock_now()` when it is responsible for entering the lock.
3. Render the locked UI and collect unlock credentials.
4. Call `runtime.unlock(&request)`, then clear the secret.

On Wayland, `limes-core` uses `ext-session-lock-v1` through
`WaylandSessionLockBackend` to ask the compositor to secure the session. The
backend keeps lock surfaces alive while the frontend owns the user-facing lock UI.

## Example

Configure `/etc/pam.d/limes` before testing PAM-backed auth. Then run the lock
frontend example under a Wayland compositor with `ext-session-lock-v1` support:

```sh
cargo run -p limes-simple-lock -- lock
```

Session choices are provided by `limes-core` from system `.desktop` files in
`wayland-sessions` and `xsessions`. Extra backend session entries can be supplied
with a semicolon-separated list:

```sh
export LIMES_SESSIONS='Lab Shell=/bin/sh;Sway=sway'
```

## Development

```sh
cargo fmt --all
cargo test --workspace
```

## Acknowledgements

The direct PAM login/session flow is informed by [Ly](https://github.com/fairyglade/ly):
`pam_start`, `PAM_TTY`, `pam_authenticate`, `pam_acct_mgmt`, `pam_setcred`,
`pam_open_session`, PAM environment import, user context switch, and parent-side
session waiting/cleanup. Before starting a new PAM auth challenge, `limes-core`
cleans up any prior PAM transaction that has not yet been opened as a login
session; already-opened sessions remain owned by the returned session handle and
are closed during normal session cleanup. The lock authentication path follows
[swaylock](https://github.com/swaywm/swaylock)'s model of a small PAM
conversation that answers password prompts and maps PAM errors into
frontend-renderable failures.
