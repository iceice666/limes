# limes

Log In Manager & Screenlock.

`limes` is intended to ship as a CLI:

```sh
limes login   # called after boot by a service/DM unit
limes lock    # lock the current session
```

## Architecture

- `crates/limes-cli`: final CLI binary named `limes`.
- `crates/limes-core`: backend library for auth, PAM/session boundaries, lock state,
  session launch, frontend orchestration, config, and events.
- `crates/limes-proto`: shared types/events used by frontends and backend code.
- `examples/limes-frontend-native`: starter frontend executable and auth-process
  example. Its login path uses a small text UI, and its lock path uses an iced
  UI; both link to `limes-core` instead of owning PAM/session logic themselves.
- `examples/limes-frontend-iced`: iced-rs login screen with a tinted glass design
  over `examples/limes-frontend-iced/assets/bg.jpg`, idle lock state, password timeout, session selector, loading
  animation, and failed-auth shake feedback.

The security-sensitive path should stay in `limes-core`. Frontends should render
UI, collect credentials, and call backend APIs.

## Auth process example

Use `examples/limes-frontend-native/` as the reference for frontend-owned UI with
backend-owned authentication:

1. Build a `Runtime` from environment/config with `Runtime::from_env()`.
2. Collect username/password or PAM response in the frontend UI.
3. Create an `AuthRequest` with `username`, `password`, and optional `tty`.
4. Call `runtime.authenticate(&request)` for login verification, then clear the
   secret with `request.clear_secret()`. For the PAM backend, each new auth
   challenge first drops any previous authenticated-but-unopened PAM transaction
   so prompts start with a fresh PAM handle.
5. On successful login, call `runtime.start_session_for(&success)`, wait with
   `runtime.wait_session(&handle)`, and let `limes-core` handle PAM session
   open/close plus user context switching.

For lock/unlock UI, the same crate shows how to subscribe to PAM prompt events
with `runtime.events().subscribe(...)`, collect a password or empty response for
fingerprint/PAM flows, and verify via the backend while keeping secret handling
out of the renderer logic.

## Development smoke test

The default auth backend is PAM (`LIMES_PAM_SERVICE=limes`). Install a matching
`/etc/pam.d/limes` policy first. For local testing only, bypass PAM with:

```sh
export LIMES_AUTH_BACKEND=dev
export LIMES_DEV_PASSWORD=secret
export LIMES_SESSION_COMMAND="sh -c true"
cargo run -p limes-cli -- login --builtin
```

External frontend launch examples:

```sh
cargo run -p limes-cli -- login --frontend target/debug/limes-frontend-native -- login

# Full-screen by default. Set LIMES_ICED_WINDOWED=1 for a normal debug window.
cargo build -p limes-frontend-iced
LIMES_ICED_WINDOWED=1 cargo run -p limes-cli -- login --frontend target/debug/limes-frontend-iced -- login
```

The iced frontend closes its login window after a successful verification once
the session is started, then waits for the session to exit so `limes-core` can
close the backend auth/PAM session.

Session choices are provided by `limes-core` from system `.desktop` files in
`wayland-sessions`/`xsessions`. Extra backend session entries can be supplied
with a semicolon-separated list:

```sh
export LIMES_SESSIONS='Lab Shell=/bin/sh;Sway=sway'
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
