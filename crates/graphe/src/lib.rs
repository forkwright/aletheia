#![deny(missing_docs)]
//! aletheia-graphe: session persistence layer
//!
//! Graphe (Γραφή): "writing, record." Manages sessions, messages, usage
//! tracking, and agent portability via embedded `SQLite`.

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use aletheia_eidos::id;
/// Knowledge graph domain types (re-exported from `eidos`).
pub use aletheia_eidos::knowledge;

/// Database backup and JSON export for session data.
#[cfg(feature = "sqlite")]
pub mod backup;
/// Graphe-specific error types and result alias.
pub mod error;
/// Agent export: build an `AgentFile` from session store and workspace.
#[cfg(feature = "sqlite")]
pub mod export;
/// Agent import: restore an agent from a portable `AgentFile`.
#[cfg(feature = "sqlite")]
pub mod import;
/// Versioned `SQLite` schema migration runner.
#[cfg(feature = "sqlite")]
pub mod migration;
/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
#[cfg(feature = "sqlite")]
pub mod portability;
/// SQLite corruption detection, read-only fallback, and auto-repair.
#[cfg(feature = "sqlite")]
pub mod recovery;
/// Session retention policies and automated cleanup of old data.
#[cfg(feature = "sqlite")]
pub mod retention;
/// `SQLite` schema DDL constants.
#[cfg(feature = "sqlite")]
pub mod schema;
/// `SQLite` session store (WAL mode, prepared statements, transactional writes).
#[cfg(feature = "sqlite")]
pub mod store;
/// Core types for sessions, messages, usage records, and agent notes.
pub mod types;

#[cfg(all(test, feature = "sqlite"))]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::store::SessionStore;

    assert_impl_all!(SessionStore: Send);
}
