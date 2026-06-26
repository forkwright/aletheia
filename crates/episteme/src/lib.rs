#![deny(missing_docs)]
//! aletheia-episteme: knowledge pipeline
//!
//! Episteme (Ἐπιστήμη): "knowledge, understanding." Extraction, storage,
//! recall, and maintenance of the knowledge graph.

/// Datalog/graph engine (provided by `aletheia-krites`).
#[cfg(feature = "mneme-engine")]
pub use krites as engine;

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use eidos::id;
/// Knowledge graph domain types (re-exported from `eidos`).
pub use eidos::knowledge;
/// Error types for knowledge operations.
pub mod error;

/// Memory admission control: structured decision gate for knowledge graph insertion.
pub mod admission;
/// Bookkeeping provider implementations for extraction and classifier work.
pub mod bookkeeping;
/// Causal edge store: directed graph of causal relationships between facts.
pub mod causal;
/// Conflict detection pipeline for fact insertion.
pub mod conflict;
/// LLM-driven fact consolidation for knowledge maintenance.
pub mod consolidation;
/// Multi-factor temporal decay with lifecycle stages and graduated pruning.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "pub(crate) items used only in tests")
)]
pub mod decay;
/// Entity deduplication pipeline for merging semantically identical entities.
///
/// Public surface is intentionally minimal: only the operator-tunable
/// [`DedupTuning`](dedup::DedupTuning) struct and its default constants
/// are exposed so the binary crate can build a tuning from
/// `taxis::AgentBehaviorDefaults` and pass it into the dedup entry
/// points on [`KnowledgeStore`](crate::knowledge_store::KnowledgeStore)
/// (#4165 D). All internal types remain `pub(crate)`.
pub mod dedup;
/// Derived Datalog rule definitions: ontological IS-A closure, causal chains,
/// and defeasible defaults. Rule strings consumed by the knowledge store's
/// derived-rule materialization engine.
pub mod derived_rules;
/// Embedding provider trait and implementations (candle, mock).
pub mod embedding;
/// Embedding evaluation gate: Recall@K and MRR for model upgrade checks.
pub mod embedding_eval;
/// Evidence-gap tracking for iterative retrieval (MemR3-inspired).
pub mod evidence_gap;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub mod extract;
/// Graph-enhanced recall scoring: PageRank boost, community proximity, supersession chains.
pub mod graph_intelligence;
/// Data source ingestion pipeline: file → chunk → fact extraction.
pub mod ingest;
/// Instinct system: behavioral memory from tool usage patterns.
pub mod instinct;
/// Knowledge graph export/import for agent portability.
pub mod knowledge_portability;
/// Krites-backed knowledge store for graph traversal and vector search.
pub mod knowledge_store;
/// Memory manifest: lightweight headers for side-query pre-filtering.
pub mod manifest;
/// Prometheus metric definitions for the knowledge pipeline.
pub mod metrics;
/// Operational fact extraction: runtime metrics as knowledge graph facts.
pub mod ops_facts;
/// Typed Datalog query builder for compile-time schema validation.
#[cfg(feature = "mneme-engine")]
pub mod query;
/// LLM-powered query rewriting for recall pipeline enhancement.
pub mod query_rewrite;
/// 11-factor recall scoring engine for knowledge retrieval ranking.
pub mod recall;
/// Reinforcement-learning readiness types for future memory-policy training.
pub mod rl;
/// Steward rule proposal generation from observed tool-usage patterns.
pub mod rule_proposals;
/// Serendipity engine for discovery and unexpected connection finding.
pub(crate) mod serendipity;
/// Side-query memory relevance selector with LRU caching and already-surfaced tracking.
pub mod side_query;
/// Skill storage helpers and SKILL.md parser.
pub mod skill;
/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
pub mod skills;
/// Source-linked re-fetching for fact staleness validation.
pub mod staleness;
/// Ecological succession: domain volatility tracking and adaptive decay rates.
pub(crate) mod succession;
/// Bayesian surprise for episode boundary detection (EM-LLM, arXiv 2407.09450).
pub mod surprise;
/// Structured tracing subscriber that captures operational events as Datalog facts.
pub mod trace_ingest;
/// Relationship type normalization and validation for knowledge graph extraction.
/// Multi-agent verification protocol: publish, vote, and conflict resolution.
pub mod verification;

pub mod vocab;

/// Internal similarity and helper utilities shared across episteme modules.
pub(crate) mod utils;

/// Shared test fixtures for knowledge store tests (DRY helpers).
#[cfg(all(test, feature = "mneme-engine"))]
pub(crate) mod test_fixtures;

#[cfg(test)]
mod phase_f_integration_tests;
#[cfg(test)]
mod succession_tests;
