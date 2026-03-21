#![deny(missing_docs)]
//! aletheia-mneme: session store and memory engine
//!
//! Mneme (Μνήμη): "memory." Thin facade that re-exports from the extracted
//! sub-crates: graphe (session persistence), episteme (knowledge pipeline),
//! eidos (types), and krites (Datalog engine).

// ── Types (eidos) ──────────────────────────────────────────────────────────

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use aletheia_eidos::id;
/// Knowledge graph domain types: facts, entities, relationships, embeddings (re-exported from `eidos`).
pub use aletheia_eidos::knowledge;

// ── Engine (krites) ────────────────────────────────────────────────────────

/// Datalog/graph engine (enabled by `mneme-engine` feature, provided by `aletheia-krites`).
#[cfg(feature = "mneme-engine")]
pub use aletheia_krites as engine;

// ── Session persistence (graphe) ───────────────────────────────────────────

/// Database backup and JSON export for session data.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::backup;
/// Mneme-specific error types and result alias.
pub use aletheia_graphe::error;
/// Agent export: build an `AgentFile` from session store and workspace.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::export;
/// Agent import: restore an agent from a portable `AgentFile`.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::import;
/// Versioned `SQLite` schema migration runner.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::migration;
/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::portability;
/// `SQLite` corruption detection, read-only fallback, and auto-repair.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::recovery;
/// Session retention policies and automated cleanup of old data.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::retention;
/// `SQLite` schema DDL constants.
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::schema;
/// `SQLite` session store (WAL mode, prepared statements, transactional writes).
#[cfg(feature = "sqlite")]
pub use aletheia_graphe::store;
/// Core types for sessions, messages, usage records, and agent notes.
pub use aletheia_graphe::types;

// ── Knowledge pipeline (episteme) ──────────────────────────────────────────

/// Conflict detection pipeline for fact insertion.
pub use aletheia_episteme::conflict;
/// LLM-driven fact consolidation for knowledge maintenance.
pub use aletheia_episteme::consolidation;
/// Embedding provider trait and implementations (candle, mock).
pub use aletheia_episteme::embedding;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub use aletheia_episteme::extract;
/// In-memory HNSW vector index backed by `hnsw_rs`.
#[cfg(feature = "hnsw_rs")]
pub use aletheia_episteme::hnsw_index;
/// Instinct system: behavioral memory from tool usage patterns.
pub use aletheia_episteme::instinct;
/// Knowledge graph export/import for agent portability.
pub use aletheia_episteme::knowledge_portability;
/// `CozoDB`-backed knowledge store for graph traversal and vector search.
pub use aletheia_episteme::knowledge_store;
/// Typed Datalog query builder for compile-time schema validation.
#[cfg(feature = "mneme-engine")]
pub use aletheia_episteme::query;
/// 6-factor recall scoring engine for knowledge retrieval ranking.
pub use aletheia_episteme::recall;
/// Skill storage helpers and SKILL.md parser.
pub use aletheia_episteme::skill;
/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
pub use aletheia_episteme::skills;
/// Relationship type normalization and validation for knowledge graph extraction.
pub use aletheia_episteme::vocab;

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
        let ks = KnowledgeStore::open_mem()
            .expect("KnowledgeStore::open_mem should succeed with mneme-engine feature");

        let ss = SessionStore::open_in_memory()
            .expect("SessionStore::open_in_memory should succeed with sqlite feature");

        drop(ks);
        drop(ss);
    }
}
