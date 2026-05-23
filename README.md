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
- `crates/limes-frontend-native`: starter frontend executable. It is currently a
  text renderer, but models how a native/webview frontend can link to
  `limes-core` and avoid owning auth logic itself.

The security-sensitive path should stay in `limes-core`. Frontends should render
UI, collect credentials, and call backend APIs.

## Development smoke test

The default auth backend is PAM (`LIMES_PAM_SERVICE=limes`). Install a matching
`/etc/pam.d/limes` policy first. For local testing only, bypass PAM with:

```sh
export LIMES_AUTH_BACKEND=dev
export LIMES_DEV_PASSWORD=secret
export LIMES_SESSION_COMMAND="sh -c true"
cargo run -p limes-cli -- login --builtin
```

External frontend launch example:

```sh
cargo run -p limes-cli -- login --frontend target/debug/limes-frontend-native -- login
```

## Acknowledgements

The direct PAM login/session flow is informed by [Ly](https://github.com/fairyglade/ly):
`pam_start`, `PAM_TTY`, `pam_authenticate`, `pam_acct_mgmt`, `pam_setcred`,
`pam_open_session`, PAM environment import, user context switch, and parent-side
session waiting/cleanup. The lock authentication path follows [swaylock](https://github.com/swaywm/swaylock)'s
model of a small PAM conversation that answers password prompts and maps PAM
errors into frontend-renderable failures.
