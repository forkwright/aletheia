//! aletheia-mneme — session store and memory engine
//!
//! Mneme (Μνήμη) — "memory." Manages sessions, messages, and usage tracking
//! via embedded `SQLite` (`rusqlite`). Future: `CozoDB` for vectors + graph.
//!
//! Depends on `aletheia-koina` for types and errors.

#[cfg(feature = "mneme-engine")]
#[expect(
    unsafe_code,
    dead_code,
    private_interfaces,
    unexpected_cfgs,
    unused_imports,
    clippy::pedantic,
    clippy::mutable_key_type,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::non_canonical_partial_ord_impl,
    clippy::neg_cmp_op_on_partial_ord,
    reason = "absorbed CozoDB engine code — refactoring deferred to Phase E"
)]
pub mod engine;

/// Database backup and JSON export for session data.
#[cfg(feature = "sqlite")]
pub mod backup;
/// Embedding provider trait and implementations (fastembed, mock).
pub mod embedding;
/// Mneme-specific error types and result alias.
pub mod error;
/// Agent export — build an `AgentFile` from session store and workspace.
#[cfg(feature = "sqlite")]
pub mod export;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub mod extract;
/// Agent import — restore an agent from a portable `AgentFile`.
#[cfg(feature = "sqlite")]
pub mod import;
/// Knowledge graph domain types: facts, entities, relationships, embeddings.
pub mod knowledge;
/// `CozoDB`-backed knowledge store for graph traversal and vector search.
pub mod knowledge_store;
/// Versioned `SQLite` schema migration runner.
#[cfg(feature = "sqlite")]
pub mod migration;
/// Agent portability schema — `AgentFile` format for cross-runtime export/import.
#[cfg(feature = "sqlite")]
pub mod portability;
/// Typed Datalog query builder for compile-time schema validation.
#[cfg(feature = "mneme-engine")]
pub mod query;
/// 6-factor recall scoring engine for knowledge retrieval ranking.
pub mod recall;
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
    use super::store::SessionStore;
    use static_assertions::assert_impl_all;

    assert_impl_all!(SessionStore: Send);
}
