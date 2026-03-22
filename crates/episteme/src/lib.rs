#![deny(missing_docs)]
//! aletheia-episteme: knowledge pipeline
//!
//! Episteme (Ἐπιστήμη): "knowledge, understanding." Extraction, storage,
//! recall, and maintenance of the knowledge graph.

/// Datalog/graph engine (provided by `aletheia-krites`).
#[cfg(feature = "mneme-engine")]
pub use aletheia_krites as engine;

/// Newtype wrappers for knowledge-domain identifiers (re-exported from `eidos`).
pub use aletheia_eidos::id;
/// Knowledge graph domain types (re-exported from `eidos`).
pub use aletheia_eidos::knowledge;
/// Error types (re-exported from `graphe`).
pub use aletheia_graphe::error;

/// Conflict detection pipeline for fact insertion.
pub mod conflict;
/// LLM-driven fact consolidation for knowledge maintenance.
pub mod consolidation;
/// Multi-factor temporal decay with lifecycle stages and graduated pruning.
pub mod decay;
/// Entity deduplication pipeline for merging semantically identical entities.
pub(crate) mod dedup;
/// Embedding provider trait and implementations (candle, mock).
pub mod embedding;
/// LLM-driven knowledge extraction pipeline (entities, relationships, facts).
pub mod extract;
/// Graph-enhanced recall scoring: PageRank boost, community proximity, supersession chains.
pub(crate) mod graph_intelligence;
/// In-memory HNSW vector index backed by `hnsw_rs`.
#[cfg(feature = "hnsw_rs")]
pub mod hnsw_index;
/// Instinct system: behavioral memory from tool usage patterns.
pub mod instinct;
/// Knowledge graph export/import for agent portability.
pub mod knowledge_portability;
/// `CozoDB`-backed knowledge store for graph traversal and vector search.
pub mod knowledge_store;
/// Typed Datalog query builder for compile-time schema validation.
#[cfg(feature = "mneme-engine")]
pub mod query;
/// LLM-powered query rewriting for recall pipeline enhancement.
pub(crate) mod query_rewrite;
/// 6-factor recall scoring engine for knowledge retrieval ranking.
pub mod recall;
/// Serendipity engine: cross-domain discovery, surprise scoring, and context injection.
pub mod serendipity;
/// Skill storage helpers and SKILL.md parser.
pub mod skill;
/// Skill auto-capture: heuristic filter, signature hashing, and candidate tracking.
pub mod skills;
/// Ecological succession: domain volatility tracking and adaptive decay rates.
pub(crate) mod succession;
/// Relationship type normalization and validation for knowledge graph extraction.
pub mod vocab;

#[cfg(test)]
mod phase_f_integration_tests;
#[cfg(test)]
mod succession_tests;
