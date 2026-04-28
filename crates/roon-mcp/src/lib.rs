//! Library surface for `roon-mcp`.
//!
//! Re-exposes internal modules so integration tests under `tests/` can
//! reuse the same types as the binary entry point in `src/main.rs`.

pub mod auth;
