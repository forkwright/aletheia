//! Binary-only modules.
//!
//! Files under `commands/` are part of the binary surface and use
//! `println!` directly (kanon's `RUST/println-in-lib` rule skips
//! `commands/` for this reason; the binary's stdout is intentionally
//! human-facing).

pub mod report;
