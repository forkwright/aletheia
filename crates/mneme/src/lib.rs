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
//!
//! # Why this crate exists (facade justification)
//!
//! Mneme is a pure re-export layer with no logic of its own (~300 lines).
//! It earns its place through three concrete benefits:
//!
//! 1. **API stability**: downstream application crates import from `mneme`
//!    instead of from `eidos`/`graphe`/`episteme`/`krites` directly. If
//!    sub-crates are reorganized, downstream `use` statements do not change.
//!
//! 2. **Feature gating**: mneme gates `krites` behind the `mneme-engine`
//!    feature flag. Without the facade, every consuming crate would need to
//!    duplicate this feature gate in its own `Cargo.toml`.
//!
//! 3. **Import ergonomics**: a single `mneme::` prefix replaces four crate
//!    prefixes, reducing cognitive overhead for contributors who do not need
//!    to know about the internal decomposition.
//!
//! **Alarm threshold**: if this file exceeds 500 lines, the facade is accreting
//! logic that belongs in a sub-crate. Audit and extract.

// ── Types (eidos) ──────────────────────────────────────────────────────────

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use eidos::id;

/// Curated knowledge domain types: facts, entities, relationships, embeddings.
///
/// This is an explicit, curated subset of `eidos::knowledge` rather than a
/// whole-module re-export (#4553). The facade surfaces only the domain types
/// downstream consumers import; storage-oriented helpers — notably the
/// flat-file [`FactStore`](eidos::knowledge::architecture_fact::FactStore) in
/// `eidos::knowledge::architecture_fact` — are intentionally excluded so the
/// public memory boundary stays a curated contract and storage internals can
/// evolve without leaking through the facade. Add a name here when a
/// downstream crate genuinely needs it; reach into `eidos` directly for
/// internal/storage types.
pub mod knowledge {
    pub use eidos::knowledge::{
        ConflictResolution, EmbeddedChunk, Entity, EpistemicTier, Fact, FactAccess, FactLifecycle,
        FactProvenance, FactSensitivity, FactTemporal, FactType, ForgetReason, MemoryScope,
        RecallResult, Relationship, VerificationProposal, VerificationVerdict, VerificationVote,
        Visibility, default_stability_hours, far_future, format_timestamp, parse_timestamp,
    };
}
/// Workspace/project identity primitives (re-exported from `eidos`).
pub mod workspace {
    pub use eidos::workspace::{ProjectId, ProjectIdError};
}

/// Bookkeeping provider contracts, DTOs, and pipeline adapters.
pub mod bookkeeping {
    pub use eidos::bookkeeping::{
        BookkeepingError, BookkeepingProvider, BookkeepingResult, ConversationMessage, EntityType,
        ExtractedEntity, ExtractedFact, ExtractedRelationship, ExtractedToolCall, Extraction,
        ExtractionSchema, Intent,
    };
    pub use episteme::bookkeeping::LlmBookkeepingProvider;
    #[cfg(feature = "gliner")]
    pub use episteme::bookkeeping::{GlinerExtractionProvider, GlinerProviderConfig};
}

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

/// Benchmark isolation and evidence primitives for eval harnesses.
#[cfg(feature = "mneme-engine")]
pub mod benchmark;

// ── Session persistence (graphe) ───────────────────────────────────────────

/// Mneme-specific error types and result alias.
pub mod error {
    pub use graphe::error::Error;
}

/// Knowledge-domain error types and result alias (re-exported from `episteme`).
pub mod knowledge_error {
    pub use episteme::error::{Error, Result};
}

/// Agent portability schema: `AgentFile` format for cross-runtime export/import.
pub mod portability {
    pub use graphe::portability::{
        AGENT_FILE_VERSION, AgentFile, BinaryFileData, ExportMetadata, ExportedMessage,
        ExportedNote, ExportedSession, ExportedUsageRecord, ExportedVector, FactEntityEdge,
        GraphData, KnowledgeExport, MemoryData, NousInfo, OmittedSection, TruncationRecord,
        WorkspaceData,
    };
}

/// Session store — fjall LSM-tree backend.
pub mod store {
    pub use graphe::store::{
        FinalizeMessage, FinalizeNote, FinalizeToolAuditRecord, FinalizeTurnRequest,
        FinalizeTurnResult, SessionStore,
    };
}

/// Core types for sessions, messages, usage records, and agent notes.
pub mod types {
    pub use graphe::types::{
        AgentNote, BlackboardRow, Message, Role, Session, SessionMetrics, SessionOrigin,
        SessionStatus, SessionType, ToolAuditRecord, UsageRecord,
    };
    pub use graphe::types::{
        ReservedIdPrefixError, ReservedIdPrefixSnafu, ValidatedId, is_reserved_session_prefix,
        parse_session_or_agent_id,
    };
}

/// Idempotent turn-finalization primitives.
///
/// WHY: finalized turns must carry durable lifecycle evidence so downstream
/// recovery can distinguish pending attempts from completed turns (#4691).
pub mod finalize;

/// Working-memory checkpoint storage contract.
///
/// WHY: the working-checkpoint hook is enabled by default but the runtime
/// passes no store, leaving the hook/tool as no-ops. This module defines the
/// storage surface so a downstream runtime can wire a durable backend
/// through the memory boundary (#4688).
pub mod checkpoint;

// ── Training data types (eidos) ───────────────────────────────────────
//
// NOTE: training capture *logic* (the JSONL writer, quality gate, and
// `TrainingCapture` struct) lives in `nous::training` — it is a pipeline
// tap, not a memory operation. Mneme re-exports only the shared types
// that the configuration layer needs.

/// Training data types re-exported from eidos.
pub mod training {
    pub use eidos::training::{
        RecallSignals, RecalledFact, TRAINING_RECORD_SCHEMA_VERSION, ToolOutcome, TrainingConfig,
        TrainingRecord,
    };
}

/// Finding types for attention-quality audits and eval reports.
pub mod finding {
    pub use eidos::knowledge::finding::{
        ConfidenceSummary, EvidenceLevel, EvidenceRef, Finding, FindingStats, FindingSupport,
        stable_hash,
    };
}

/// Metadata provenance primitives for stamped artefacts.
pub mod meta {
    pub use eidos::meta::{ArtefactMeta, Provenance, ProvenanceProject, Stamped};
}

// ── Knowledge pipeline (episteme) ──────────────────────────────────────────

/// LLM-driven fact consolidation for knowledge maintenance.
pub mod consolidation {
    pub use episteme::consolidation::ConsolidationConfig;
}

/// Embedding provider trait and implementations (candle, mock).
pub mod embedding {
    pub use episteme::embedding::{
        DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingError, EmbeddingProvider,
        create_provider, is_degraded_provider,
    };

    #[cfg(any(test, feature = "test-support"))]
    pub use episteme::embedding::MockEmbeddingProvider;
}

/// Embedding evaluation gate: Recall@K and MRR for model upgrade checks.
pub mod embedding_eval {
    pub use episteme::embedding_eval::{EvalDataset, EvalRunResult, compare_models};
}

/// Data source ingestion pipeline: file → chunk → fact extraction.
pub mod ingest {
    pub use episteme::ingest::{
        IngestChunk, IngestConfig, IngestFormat, ingest_content, parse_format,
    };
}

/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub mod extract {
    pub use episteme::extract::{
        BookkeepingProviderKind, ConversationMessage, ExtractedToolCall, ExtractionConfig,
        ExtractionEngine, ExtractionError, ExtractionProvider, LlmCallSnafu, refinement,
    };
}

/// Instinct system: behavioral memory from tool usage patterns.
pub mod instinct {
    pub use episteme::instinct::{
        DEFAULT_MAX_CONTEXT_SUMMARY_LEN, DEFAULT_MAX_PARAM_VALUE_LEN,
        DEFAULT_PROMOTION_MIN_CONFIDENCE, DEFAULT_PROMOTION_MIN_PROJECTS, ToolObservation,
        ToolOutcome, sanitize_parameters, truncate_context_summary,
    };
}

/// Krites-backed knowledge store for graph traversal and vector search.
#[cfg(feature = "mneme-engine")]
pub mod knowledge_store {
    pub use episteme::knowledge_store::{
        HybridQuery, KnowledgeConfig, KnowledgeStore, QueryResult,
    };
}

/// Admission control policy types for knowledge store fact insertion.
///
/// Downstream consumers (e.g. the binary crate's runtime setup) need access
/// to the concrete policy types to wire the config-selected policy into
/// `KnowledgeConfig` without depending on `episteme` directly.
#[cfg(feature = "mneme-engine")]
pub mod admission {
    pub use episteme::admission::{
        AdmissionPolicy, DefaultAdmissionPolicy, StructuredAdmissionConfig,
        StructuredAdmissionPolicy,
    };
}

/// Entity dedup tuning (#4165 D): operator-configurable weights/thresholds
/// the CLI and scheduled maintenance task feed into the dedup pipeline,
/// plus the `DEFAULT_*` fallbacks used when no config override is available.
#[cfg(feature = "mneme-engine")]
pub mod dedup {
    pub use episteme::dedup::{
        DEFAULT_AUTO_MERGE_THRESHOLD, DEFAULT_EMBED_THRESHOLD, DEFAULT_JW_THRESHOLD,
        DEFAULT_REVIEW_THRESHOLD, DEFAULT_WEIGHT_ALIAS, DEFAULT_WEIGHT_EMBED, DEFAULT_WEIGHT_NAME,
        DEFAULT_WEIGHT_TYPE, DedupTuning,
    };
}

/// Operational metrics registration for knowledge and session storage.
pub mod metrics {
    pub use episteme::metrics::{
        record_confidence_inflation, record_embedding_duration, record_extraction,
        record_extraction_confidence, record_extraction_conflict, record_extraction_contradiction,
        record_extraction_correction, record_extraction_quality, register as register_knowledge,
    };
    pub use graphe::metrics::{record_backup_duration, register as register_sessions};
}

/// Memory manifest types used by side-query selection.
pub mod manifest {
    pub use episteme::manifest::{MemoryHeader, MemoryManifest};
}

/// Query rewriting for recall search.
pub mod query_rewrite {
    pub use episteme::query_rewrite::{
        QueryRewriter, RewriteConfig, RewriteError, RewriteProvider, RewriteResult, SearchTier,
        TieredSearchConfig, TieredSearchResult,
    };
}

/// 11-factor recall scoring engine for knowledge retrieval ranking.
pub mod recall {
    pub use episteme::recall::{
        FactorScores, ProjectRecallScope, RecallEngine, RecallWeights, ScoredResult,
        filter_by_cohort_visibility, filter_by_project_scope, filter_by_visibility,
    };

    /// Explainable recall scoring helpers for HTTP search surfaces.
    pub mod explain {
        pub use episteme::recall::explain::{
            CandidateDecision, ExplainedCandidate, RecallExplanation, explain_recall,
        };
    }

    /// Optional reranker implementations and trait.
    #[cfg(feature = "reranker")]
    pub mod reranker {
        pub use episteme::recall::reranker::{
            EpistemeError, HttpReranker, NaiveReranker, Reranker,
        };
    }
}

/// Bayesian-surprise calculator (`EM-LLM`) for topic-shift detection: a
/// session-scoped running-distribution scorer threaded into recall scoring.
pub mod surprise {
    pub use episteme::surprise::{DEFAULT_EMA_ALPHA, DEFAULT_THRESHOLD, SurpriseCalculator};
}

/// Evidence-gap tracking (`MemR3`) for iterative retrieval.
pub mod evidence_gap {
    pub use episteme::evidence_gap::{AnsweredQuestion, EvidenceGapTracker, EvidenceQuery};
}

/// Side-query memory relevance selector.
pub mod side_query {
    pub use episteme::side_query::{
        RankerFailedSnafu, SideQueryConfig, SideQueryError, SideQueryRanker, SideQueryResult,
        SideQuerySelector,
    };
}

/// Skill storage helpers and SKILL.md parser.
pub mod skill {
    pub use episteme::skill::{
        SkillContent, export_skills_to_cc, format_skill_md, parse_skill_md, scan_skill_dir,
    };
}

/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
pub mod skills {
    pub use episteme::skills::{
        CandidateTracker, PendingSkill, SkillExtractor, SkillReviewAudit, SkillReviewDecision,
        SkillReviewInput, ToolCallRecord, TrackResult,
    };

    /// Skill extraction provider types.
    pub mod extract {
        pub use episteme::skills::extract::{
            LlmCallSnafu, PendingSkill, SkillExtractionAudit, SkillExtractionError,
            SkillExtractionProvider,
        };
    }
}

/// Structured tracing subscriber that captures operational events as Datalog facts.
pub mod trace_ingest {
    pub use episteme::trace_ingest::{OPS_DDL, TraceEvent, TraceIngestLayer, ensure_ops_schema};
}

/// Multi-agent verification protocol.
pub mod verification {
    pub use episteme::verification::{
        Conflict, ConflictKind, DEFAULT_VERIFICATION_THRESHOLD, ResolveError, VerificationOutcome,
        detect_conflict, publish_fact, resolve_conflict, vote_on_proposal,
    };
}

#[cfg(test)]
mod facade_surface_tests {
    //! Pins the curated `mneme::knowledge` facade surface (#4553). Every name in
    //! the import below must stay exported; if a re-export is dropped, this stops
    //! compiling. Storage helpers such as `eidos::knowledge::architecture_fact`'s
    //! `FactStore` are deliberately absent and must never be added here.
    use core::marker::PhantomData;

    use crate::knowledge::{
        ConflictResolution, EmbeddedChunk, Entity, EpistemicTier, Fact, FactAccess, FactLifecycle,
        FactProvenance, FactSensitivity, FactTemporal, FactType, ForgetReason, MemoryScope,
        RecallResult, Relationship, VerificationProposal, VerificationVerdict, VerificationVote,
        Visibility, default_stability_hours, far_future, format_timestamp, parse_timestamp,
    };

    // kanon:ignore TESTING/tautological-test WHY: this is a compile-time surface existence check; the test "passes" when the import block above compiles successfully
    #[test]
    fn curated_knowledge_surface_is_exported() {
        // Naming each curated type pins it to the facade contract without
        // constructing values; the function items pin the curated helpers.
        let _ = PhantomData::<(
            EmbeddedChunk,
            Entity,
            EpistemicTier,
            Fact,
            FactAccess,
            FactLifecycle,
            FactProvenance,
            FactSensitivity,
            FactTemporal,
            FactType,
            ForgetReason,
            MemoryScope,
            RecallResult,
            Relationship,
            VerificationProposal,
            VerificationVerdict,
            VerificationVote,
            Visibility,
            ConflictResolution,
        )>;
        let _ = far_future;
        let _ = default_stability_hours;
        let _ = parse_timestamp;
        let _ = format_timestamp;
    }
}

#[cfg(all(test, feature = "mneme-engine"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod coexistence_tests {
    use crate::knowledge_store::KnowledgeStore;
    use crate::store::SessionStore;

    #[test]
    fn engine_and_session_store_coexist() {
        let ks = KnowledgeStore::open_mem()
            .expect("KnowledgeStore::open_mem should succeed with mneme-engine feature"); // kanon:ignore RUST/expect WHY: test assertion; infallible in-memory store

        let ss =
            SessionStore::open_in_memory().expect("SessionStore::open_in_memory should succeed"); // kanon:ignore RUST/expect WHY: test assertion; infallible in-memory store

        drop(ks);
        drop(ss);
    }
}
