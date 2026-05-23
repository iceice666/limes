# limes

Login manager and screenlock libraries for Rust frontends.

The project is split into focused crates so frontends can depend only on the
backend pieces they need:

- `limes-common`: shared PAM authentication, config, events, frontend launching,
  and error handling.
- `limes-lock`: screenlock state, unlock authentication orchestration, and the
  Wayland `ext-session-lock-v1` display backend.
- `limes-login`: login authentication orchestration, PAM session open/close,
  user session launch, and session discovery.
- `limes-proto`: lightweight shared types/events for frontends and backend code.
- `examples/simple-lock`: minimal iced/layer-shell lock frontend using
  `limes-lock` only for limes APIs.

There is no bundled CLI. Applications and examples link to the crates directly
instead of shelling out to a `limes` command.

## Frontend integration

A login frontend should:

1. Build a `LoginRuntime` from environment/config with
   `limes_login::LoginRuntime::from_env()`.
2. Collect username/password or PAM responses in the frontend UI.
3. Create an `AuthRequest` with `username`, `password`, and optional `tty`.
4. Call `runtime.authenticate(&request)`, then clear the secret with
   `request.clear_secret()`.
5. On success, call `runtime.start_session_for(&success)` or
   `runtime.start_session_for_with_command(&success, command)`, then
   `runtime.wait_session(&handle)` so `limes-login` handles PAM session
   open/close and user context switching.

A lock frontend should:

1. Build a `LockRuntime` with `limes_lock::LockRuntime::from_env()`.
2. Call `runtime.lock_now()` when it is responsible for entering the lock.
3. Render the locked UI and collect unlock credentials.
4. Call `runtime.unlock(&request)`, then clear the secret.

On Wayland, `limes-lock` uses `ext-session-lock-v1` through
`WaylandSessionLockBackend` to ask the compositor to secure the session. The
backend keeps lock surfaces alive while the frontend owns the user-facing lock UI.

## Example

Configure `/etc/pam.d/limes` before testing PAM-backed auth. Then run the lock
frontend example under a Wayland compositor with `ext-session-lock-v1` support:

```sh
cargo run -p limes-simple-lock -- lock
```

Session choices are provided by `limes-login` from system `.desktop` files in
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
session waiting/cleanup. Before starting a new PAM auth challenge, `limes-common`
cleans up any prior PAM transaction that has not yet been opened as a login
session; already-opened sessions remain owned by the returned session handle and
are closed during normal session cleanup. The lock authentication path follows
[swaylock](https://github.com/swaywm/swaylock)'s model of a small PAM
conversation that answers password prompts and maps PAM errors into
frontend-renderable failures.
