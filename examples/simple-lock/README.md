# limes simple lock (iced)

A small iced UI that drives the limes lock authentication path:

1. if launched with a `lock` arg, it calls `Runtime::lock_now()` and shows only unlock flow;
2. if run standalone, it does not auto-lock and shows a manual `Lock again` button;
3. collects password (username is filled from $USER),
4. submits an `AuthRequest` to `Runtime::unlock`, for lock/unlock authentication.
5. uses `PamAuth` and prints backend events to stderr.
6. renders as an iced layer-shell surface, so the password field can be used with Enter to unlock.

The compositor lock uses real Wayland session-lock integration. `WaylandSessionLockBackend`
keeps the lock surfaces blank while this example owns the user-facing UI and
lock/unlock flow through `limes-core`.

Run it after configuring `/etc/pam.d/limes`:

```sh
cargo run -p limes-simple-lock -- lock

# or run without auto-lock for manual testing
cargo run -p limes-simple-lock
```
