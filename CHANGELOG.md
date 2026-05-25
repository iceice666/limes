# Changelog

## 0.2.0 - 2026-05-25

### Changed

- Split long Rust source files into focused modules across `limes-proto`, `limes-common`, `limes-lock`, and `limes-login`.
- Reorganized PAM authentication internals, Wayland lock backend internals, lock manager/display traits, shared proto types, and session discovery parsing without changing runtime behavior.
- Changed some public module paths as part of the module cleanup while keeping crate-root reexports for common types.

### Added

- Added focused unit coverage for session catalog desktop command parsing and environment-provided session entries.
