//! aletheia-koina — core types, errors, and tracing for Aletheia
//!
//! Koina (κοινά) — "shared things." The common foundation that every crate depends on.
//! Imports nothing from other Aletheia crates. Contains only types, error definitions,
//! and tracing initialization.

pub mod error;
pub mod id;
pub mod tracing_init;
