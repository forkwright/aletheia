#![deny(missing_docs)]
//! aletheia-graphe: session persistence layer
//!
//! Graphe (Γραφή): "writing, record." Manages sessions, messages, and usage
//! tracking via a fjall LSM-tree store (pure Rust, zero C dependencies).

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use eidos::id;
/// Knowledge graph domain types (re-exported from `eidos`).
pub use eidos::knowledge;

/// Graphe-specific error types and result alias.
pub mod error;
/// Prometheus metric definitions for session persistence.
pub mod metrics;
/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
pub mod portability;
/// Session store — fjall LSM-tree backend.
pub mod store;
/// Core types for sessions, messages, usage records, and agent notes.
pub mod types;

/// Shared test fixtures for session store tests.
#[cfg(test)]
pub(crate) mod test_fixtures;

#[cfg(test)]
mod fjall_assertions {
    use super::store::SessionStore;

    const _: fn() = || {
        fn assert<T: Send>() {}
        assert::<SessionStore>();
    };
}
