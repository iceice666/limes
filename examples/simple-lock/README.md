# limes simple lock (iced)

A small iced UI that drives the limes lock authentication path:

1. starts in a demo locked state through `LockManager::lock_now`,
2. collects username/password,
3. submits an `AuthRequest` to `LockManager::unlock`,
4. uses `PamAuth` and prints backend events to stderr.

This is only a frontend/auth-flow demo. Its display backend is a no-op that marks
lock/unlock as successful; it is **not** a secure screen lock.

Run it after configuring `/etc/pam.d/limes`:

```sh
cargo run -p limes-simple-lock
```
