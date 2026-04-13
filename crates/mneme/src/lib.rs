#![deny(missing_docs)]
//! aletheia-mneme: session store and memory engine
//!
//! Mneme (Μνήμη): "memory." Thin facade that re-exports from the extracted
//! sub-crates: graphe (session persistence), episteme (knowledge pipeline),
//! eidos (types), and krites (Datalog engine).

// ── Types (eidos) ──────────────────────────────────────────────────────────

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use eidos::id;
/// Knowledge graph domain types: facts, entities, relationships, embeddings (re-exported from `eidos`).
pub use eidos::knowledge;

// ── Path validation (eidos) ───────────────────────────────────────────────

/// Defense-in-depth path validation for memory file operations.
///
/// All memory read/write paths MUST go through [`validate_memory_path()`]
/// to obtain a [`ValidatedPath`], which gates I/O behind the full
/// validation layer stack. `ValidatedPath` has private fields and can only
/// be constructed through validation, making bypass impossible at the type
/// level.
///
/// [`validate_memory_path()`]: knowledge::validate_memory_path
/// [`ValidatedPath`]: knowledge::ValidatedPath
pub mod path_validation {
    pub use eidos::knowledge::{
        PathValidationError, PathValidationLayer, ValidatedPath, validate_memory_path,
    };
}

// ── Engine (krites) ────────────────────────────────────────────────────────

/// Datalog/graph engine (enabled by `mneme-engine` feature, provided by `aletheia-krites`).
#[cfg(feature = "mneme-engine")]
pub use krites as engine;

// ── Session persistence (graphe) ───────────────────────────────────────────

/// Database backup and JSON export for session data.
#[cfg(feature = "sqlite")]
pub use graphe::backup;
/// Mneme-specific error types and result alias.
pub use graphe::error;
/// Agent export: build an `AgentFile` from session store and workspace.
#[cfg(feature = "sqlite")]
pub use graphe::export;
/// Agent import: restore an agent from a portable `AgentFile`.
#[cfg(feature = "sqlite")]
pub use graphe::import;
/// Versioned `SQLite` schema migration runner.
#[cfg(feature = "sqlite")]
pub use graphe::migration;
/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
#[cfg(feature = "sqlite")]
pub use graphe::portability;
/// `SQLite` corruption detection, read-only fallback, and auto-repair.
#[cfg(feature = "sqlite")]
pub use graphe::recovery;
/// Session retention policies and automated cleanup of old data.
#[cfg(feature = "sqlite")]
pub use graphe::retention;
/// `SQLite` schema DDL constants.
#[cfg(feature = "sqlite")]
pub use graphe::schema;
/// Session store — fjall (default) or `SQLite` backend.
///
/// Both backends expose the same `SessionStore` API.
#[cfg(any(feature = "fjall", feature = "sqlite"))]
pub use graphe::store;
/// Core types for sessions, messages, usage records, and agent notes.
pub use graphe::types;

// ── Training data capture ─────────────────────────────────────────────

/// Training data capture: append-only JSONL writer for conversation turns.
pub mod training;

// ── Knowledge pipeline (episteme) ──────────────────────────────────────────

/// Memory admission control: structured decision gate for knowledge graph insertion.
pub use episteme::admission;
/// Conflict detection pipeline for fact insertion.
pub use episteme::conflict;
/// LLM-driven fact consolidation for knowledge maintenance.
pub use episteme::consolidation;
/// Multi-factor temporal decay with lifecycle stages and graduated pruning.
pub use episteme::decay;
/// Embedding provider trait and implementations (candle, mock).
pub use episteme::embedding;
/// Embedding evaluation gate: Recall@K and MRR for model upgrade checks.
pub use episteme::embedding_eval;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub use episteme::extract;
/// Instinct system: behavioral memory from tool usage patterns.
pub use episteme::instinct;
/// Knowledge graph export/import for agent portability.
pub use episteme::knowledge_portability;
/// `CozoDB`-backed knowledge store for graph traversal and vector search.
pub use episteme::knowledge_store;
/// Typed Datalog query builder for compile-time schema validation.
#[cfg(feature = "mneme-engine")]
pub use episteme::query;
/// 6-factor recall scoring engine for knowledge retrieval ranking.
pub use episteme::recall;
/// Skill storage helpers and SKILL.md parser.
pub use episteme::skill;
/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
pub use episteme::skills;
/// Relationship type normalization and validation for knowledge graph extraction.
pub use episteme::vocab;

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
