# limes simple lock (iced)

A small iced UI that drives the limes lock authentication path:

1. if launched with a `lock` arg, it calls `LockRuntime::lock_now()` and shows only unlock flow;
2. if run standalone, it does not auto-lock and shows a manual `Lock again` button;
3. collects password (username is filled from $USER),
4. submits an `AuthRequest` to `LockRuntime::unlock`, for lock/unlock authentication.
5. uses the default PAM auth exposed by `limes-lock` and prints backend events to stderr.
6. renders as an iced layer-shell surface for API/authentication experimentation.

Note: normal layer-shell surfaces are hidden once Wayland `ext-session-lock-v1`
locks the session. This example is therefore not a complete usable Wayland lock
UI when launched with `lock`; use
[`reimu_lays_on_water`](https://github.com/iceice666/reimu_lays_on_water) for a
frontend that renders directly on compositor lock surfaces.

Run it after configuring `/etc/pam.d/limes`:

```sh
cargo run -p limes-simple-lock -- lock

# or run without auto-lock for manual testing
cargo run -p limes-simple-lock
```
