#![deny(missing_docs)]
//! aletheia-mneme: session store and memory engine
//!
//! Mneme (Μνήμη): "memory." Curated facade that re-exports from the extracted
//! sub-crates: graphe (session persistence), episteme (knowledge pipeline),
//! eidos (types), and krites (Datalog engine).
//!
//! Only types that downstream consumers (nous, pylon, aletheia, melete, daemon,
//! diaporeia, integration-tests) actually import are surfaced here. Internal
//! types remain accessible through the sub-crates directly.

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
///
/// # Facade surface
///
/// [`BackupManager`](backup::BackupManager)
#[cfg(feature = "sqlite")]
pub mod backup {
    pub use graphe::backup::BackupManager;
}

/// Mneme-specific error types and result alias.
///
/// # Facade surface
///
/// [`Error`](error::Error)
pub mod error {
    pub use graphe::error::Error;
}

/// Agent export: build an `AgentFile` from session store and workspace.
///
/// # Facade surface
///
/// [`ExportOptions`](export::ExportOptions),
/// [`export_agent`](export::export_agent)
#[cfg(feature = "sqlite")]
pub mod export {
    pub use graphe::export::{ExportOptions, export_agent};
}

/// Agent import: restore an agent from a portable `AgentFile`.
///
/// # Facade surface
///
/// [`ImportOptions`](import::ImportOptions),
/// [`import_agent`](import::import_agent)
#[cfg(feature = "sqlite")]
pub mod import {
    pub use graphe::import::{ImportOptions, import_agent};
}

/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
///
/// # Facade surface
///
/// [`AgentFile`](portability::AgentFile)
#[cfg(feature = "sqlite")]
pub mod portability {
    pub use graphe::portability::AgentFile;
}

/// Session store — fjall (default) or `SQLite` backend.
///
/// Both backends expose the same `SessionStore` API.
///
/// # Facade surface
///
/// [`SessionStore`](store::SessionStore)
#[cfg(any(feature = "fjall", feature = "sqlite"))]
pub mod store {
    pub use graphe::store::SessionStore;
}

/// Core types for sessions, messages, usage records, and agent notes.
///
/// # Facade surface
///
/// [`Message`](types::Message),
/// [`Role`](types::Role),
/// [`Session`](types::Session),
/// [`SessionMetrics`](types::SessionMetrics),
/// [`SessionOrigin`](types::SessionOrigin),
/// [`SessionStatus`](types::SessionStatus),
/// [`SessionType`](types::SessionType),
/// [`UsageRecord`](types::UsageRecord)
pub mod types {
    pub use graphe::types::{
        Message, Role, Session, SessionMetrics, SessionOrigin, SessionStatus, SessionType,
        UsageRecord,
    };
}

// ── Training data types (eidos) ───────────────────────────────────────
//
// Training capture *logic* (the JSONL writer, quality gate, and
// `TrainingCapture` struct) lives in `nous::training` — it is a pipeline
// tap, not a memory operation. Mneme re-exports only the shared types
// that the configuration layer needs.

/// Training data types re-exported from eidos.
///
/// # Facade surface
///
/// [`TrainingConfig`](training::TrainingConfig),
/// [`TrainingRecord`](training::TrainingRecord),
/// [`TRAINING_RECORD_SCHEMA_VERSION`](training::TRAINING_RECORD_SCHEMA_VERSION)
pub mod training {
    pub use eidos::training::{TrainingConfig, TrainingRecord, TRAINING_RECORD_SCHEMA_VERSION};
}

// ── Knowledge pipeline (episteme) ──────────────────────────────────────────

/// LLM-driven fact consolidation for knowledge maintenance.
///
/// # Facade surface
///
/// [`ConsolidationConfig`](consolidation::ConsolidationConfig)
pub mod consolidation {
    pub use episteme::consolidation::ConsolidationConfig;
}

/// Embedding provider trait and implementations (candle, mock).
///
/// # Facade surface
///
/// [`EmbeddingProvider`](embedding::EmbeddingProvider),
/// [`MockEmbeddingProvider`](embedding::MockEmbeddingProvider),
/// [`DegradedEmbeddingProvider`](embedding::DegradedEmbeddingProvider),
/// [`EmbeddingConfig`](embedding::EmbeddingConfig),
/// [`create_provider`](embedding::create_provider)
pub mod embedding {
    pub use episteme::embedding::{
        DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
    };

    #[cfg(any(test, feature = "test-support"))]
    pub use episteme::embedding::MockEmbeddingProvider;
}

/// Embedding evaluation gate: Recall@K and MRR for model upgrade checks.
///
/// # Facade surface
///
/// [`EvalDataset`](embedding_eval::EvalDataset),
/// [`EvalRunResult`](embedding_eval::EvalRunResult),
/// [`compare_models`](embedding_eval::compare_models)
pub mod embedding_eval {
    pub use episteme::embedding_eval::{EvalDataset, EvalRunResult, compare_models};
}

/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
///
/// # Facade surface
///
/// [`ConversationMessage`](extract::ConversationMessage),
/// [`ExtractionConfig`](extract::ExtractionConfig),
/// [`ExtractionEngine`](extract::ExtractionEngine),
/// [`ExtractionError`](extract::ExtractionError),
/// [`ExtractionProvider`](extract::ExtractionProvider),
/// [`LlmCallSnafu`](extract::LlmCallSnafu)
pub mod extract {
    pub use episteme::extract::{
        ConversationMessage, ExtractionConfig, ExtractionEngine, ExtractionError,
        ExtractionProvider, LlmCallSnafu,
    };
}

/// Instinct system: behavioral memory from tool usage patterns.
///
/// # Facade surface
///
/// [`DEFAULT_MAX_CONTEXT_SUMMARY_LEN`](instinct::DEFAULT_MAX_CONTEXT_SUMMARY_LEN),
/// [`DEFAULT_MAX_PARAM_VALUE_LEN`](instinct::DEFAULT_MAX_PARAM_VALUE_LEN),
/// [`ToolObservation`](instinct::ToolObservation),
/// [`ToolOutcome`](instinct::ToolOutcome),
/// [`sanitize_parameters`](instinct::sanitize_parameters),
/// [`truncate_context_summary`](instinct::truncate_context_summary)
pub mod instinct {
    pub use episteme::instinct::{
        DEFAULT_MAX_CONTEXT_SUMMARY_LEN, DEFAULT_MAX_PARAM_VALUE_LEN, ToolObservation,
        ToolOutcome, sanitize_parameters, truncate_context_summary,
    };
}

/// `CozoDB`-backed knowledge store for graph traversal and vector search.
///
/// # Facade surface
///
/// [`HybridQuery`](knowledge_store::HybridQuery),
/// [`KnowledgeConfig`](knowledge_store::KnowledgeConfig),
/// [`KnowledgeStore`](knowledge_store::KnowledgeStore)
#[cfg(feature = "mneme-engine")]
pub mod knowledge_store {
    pub use episteme::knowledge_store::{HybridQuery, KnowledgeConfig, KnowledgeStore};
}

/// 6-factor recall scoring engine for knowledge retrieval ranking.
///
/// # Facade surface
///
/// [`FactorScores`](recall::FactorScores),
/// [`RecallEngine`](recall::RecallEngine),
/// [`RecallWeights`](recall::RecallWeights),
/// [`ScoredResult`](recall::ScoredResult)
pub mod recall {
    pub use episteme::recall::{FactorScores, RecallEngine, RecallWeights, ScoredResult};
}

/// Skill storage helpers and SKILL.md parser.
///
/// # Facade surface
///
/// [`SkillContent`](skill::SkillContent),
/// [`export_skills_to_cc`](skill::export_skills_to_cc),
/// [`parse_skill_md`](skill::parse_skill_md),
/// [`scan_skill_dir`](skill::scan_skill_dir)
pub mod skill {
    pub use episteme::skill::{SkillContent, export_skills_to_cc, parse_skill_md, scan_skill_dir};
}

/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
///
/// # Facade surface
///
/// [`CandidateTracker`](skills::CandidateTracker),
/// [`PendingSkill`](skills::PendingSkill),
/// [`SkillExtractor`](skills::SkillExtractor),
/// [`ToolCallRecord`](skills::ToolCallRecord),
/// [`TrackResult`](skills::TrackResult)
///
/// Also re-exports the `extract` submodule for skill extraction provider types.
pub mod skills {
    pub use episteme::skills::{
        CandidateTracker, PendingSkill, SkillExtractor, ToolCallRecord, TrackResult,
    };

    /// Skill extraction provider types.
    ///
    /// # Facade surface
    ///
    /// [`LlmCallSnafu`](extract::LlmCallSnafu),
    /// [`PendingSkill`](extract::PendingSkill),
    /// [`SkillExtractionError`](extract::SkillExtractionError),
    /// [`SkillExtractionProvider`](extract::SkillExtractionProvider)
    pub mod extract {
        pub use episteme::skills::extract::{
            LlmCallSnafu, PendingSkill, SkillExtractionError, SkillExtractionProvider,
        };
    }
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
        let ks = KnowledgeStore::open_mem()
            .expect("KnowledgeStore::open_mem should succeed with mneme-engine feature");

        let ss = SessionStore::open_in_memory()
            .expect("SessionStore::open_in_memory should succeed with sqlite feature");

        drop(ks);
        drop(ss);
    }
}
