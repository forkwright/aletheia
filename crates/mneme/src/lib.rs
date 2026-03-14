//! aletheia-mneme — session store and memory engine
//!
//! Mneme (Μνήμη) — "memory." Manages sessions, messages, and usage tracking
//! via embedded `SQLite` and the Datalog knowledge engine.
//!
//! Depends on `aletheia-koina` for types and errors.

#[cfg(feature = "mneme-engine")]
pub mod engine;

/// Database backup and JSON export for session data.
#[cfg(feature = "sqlite")]
pub mod backup;
/// Conflict detection pipeline for fact insertion.
pub mod conflict;
/// LLM-driven fact consolidation for knowledge maintenance.
pub mod consolidation;
/// Entity deduplication pipeline for merging semantically identical entities.
pub mod dedup;
/// Embedding provider trait and implementations (candle, mock).
pub mod embedding;
/// Mneme-specific error types and result alias.
pub mod error;
/// Agent export — build an `AgentFile` from session store and workspace.
#[cfg(feature = "sqlite")]
pub mod export;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub mod extract;
/// Graph-enhanced recall scoring: PageRank boost, community proximity, supersession chains.
pub mod graph_intelligence;
/// In-memory HNSW vector index backed by `hnsw_rs`.
#[cfg(feature = "hnsw_rs")]
pub mod hnsw_index;
/// Newtype wrappers for mneme-local domain identifiers.
pub mod id;
/// Agent import — restore an agent from a portable `AgentFile`.
#[cfg(feature = "sqlite")]
pub mod import;
/// Instinct system — behavioral memory from tool usage patterns.
pub mod instinct;
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
/// LLM-powered query rewriting for recall pipeline enhancement.
pub mod query_rewrite;
/// 6-factor recall scoring engine for knowledge retrieval ranking.
pub mod recall;
/// Session retention policies and automated cleanup of old data.
#[cfg(feature = "sqlite")]
pub mod retention;
/// `SQLite` schema DDL constants.
#[cfg(feature = "sqlite")]
pub mod schema;
/// Skill storage helpers and SKILL.md parser.
pub mod skill;
/// Skill auto-capture — heuristic filter, signature hashing, and candidate tracking.
pub mod skills;
/// `SQLite` session store (WAL mode, prepared statements, transactional writes).
#[cfg(feature = "sqlite")]
pub mod store;
/// Ecological succession — domain volatility tracking and adaptive decay rates.
pub mod succession;
/// Core types for sessions, messages, usage records, and agent notes.
pub mod types;
/// Controlled relationship type vocabulary for knowledge graph validation.
pub mod vocab;

#[cfg(test)]
mod succession_tests;

#[cfg(all(test, feature = "sqlite"))]
mod assertions {
    use super::store::SessionStore;
    use static_assertions::assert_impl_all;

    assert_impl_all!(SessionStore: Send);
}

/// Verify that `mneme-engine` and `sqlite` features coexist without `SQLite`
/// link conflicts.
///
/// The engine's `storage-sqlite` backend was removed; its remaining backends
/// (mem, redb, fjall) have no `SQLite` dependency. `rusqlite` compiles with
/// `features = ["bundled"]`, so its symbols are isolated. Both features can
/// be active in the same binary.
#[cfg(all(test, feature = "sqlite", feature = "mneme-engine"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod coexistence_tests {
    use crate::knowledge_store::KnowledgeStore;
    use crate::store::SessionStore;

    #[test]
    fn engine_and_sqlite_session_store_coexist() {
        // Open a KnowledgeStore using the mneme-engine (CozoDB, in-memory)
        let ks = KnowledgeStore::open_mem()
            .expect("KnowledgeStore::open_mem should succeed with mneme-engine feature");

        // Open a SessionStore using rusqlite (in-memory)
        let ss = SessionStore::open_in_memory()
            .expect("SessionStore::open_in_memory should succeed with sqlite feature");

        // Both instances are live in the same process — no link conflict.
        drop(ks);
        drop(ss);
    }
}
