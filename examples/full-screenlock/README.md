# limes full screenlock

A full-screen macOS liquid-glass inspired lock screen example built with iced 0.14 and `iced_sessionlock`.

It embeds `bg.jpg` as the bundled wallpaper, keeps a stable iced image handle so the background is present during redraws, and layers an animated WGSL top-down water-surface rain shader with circular ripples, ambient shimmer, long-tailed bead-like raindrops, and expanding impact rings over it. Authentication is driven through `limes-lock`.
The UI is rendered on Wayland `ext-session-lock-v1` lock surfaces, not on a normal layer-shell surface, so it remains visible while the compositor is locked.

Run the real lock mode under a Wayland compositor with `ext-session-lock-v1` support and a configured `/etc/pam.d/limes`:

```sh
cargo run -p limes-full-screenlock -- lock
```

Preview the UI in a normal resizable window without locking the session or calling PAM:

```sh
cargo run -p limes-full-screenlock -- preview
```

Behavior:

- animated WGSL top-down rain impacts, long-tailed bead-like raindrops, ambient water shimmer, and layered circular ripples over the wallpaper;
- idle state: glass-material clock at the top center;
- typing state: only a password input appears at the bottom center;
- enter submits, including empty input for PAM modules such as fingerprint auth, shrinking the input into a circular ~60 FPS flower spinner with a verification status;
- failure returns to the input with a warm error tint;
- success exits the process;
- no input for a few seconds returns to idle.
