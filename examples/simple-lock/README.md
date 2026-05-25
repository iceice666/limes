# limes simple lock (iced session lock)

A small iced UI that drives the limes lock authentication path on real Wayland
`ext-session-lock-v1` compositor lock surfaces:

1. `iced_sessionlock` owns the compositor lock surface;
2. the UI collects a password (username is filled from `$USER`);
3. submits an `AuthRequest` to `LockRuntime::authenticate_unlock`;
4. clears the secret after PAM returns;
5. asks `iced_sessionlock` to release the compositor lock after successful authentication;
6. prints backend events to stderr.

This example intentionally keeps the UI minimal. For a more polished frontend
using the same session-lock ownership pattern, see
[`reimu_lays_on_water`](https://github.com/iceice666/reimu_lays_on_water).

Run it after configuring `/etc/pam.d/limes`:

```sh
cargo run -p limes-simple-lock
```
