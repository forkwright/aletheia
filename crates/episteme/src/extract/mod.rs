//! Knowledge extraction pipeline: LLM-driven entity/relationship/fact extraction.

/// Unified diff parser for structured change analysis.
pub mod diff;
/// Dispatch pattern detection and scoring for steward learning.
pub mod dispatch;
/// Optional extraction precision/recall eval hooks for labeled fixtures.
pub mod eval;
/// Post-merge lesson extraction from PR diffs.
pub mod lesson;
/// Observation parsing from PR body markdown.
pub mod observation;
/// Context-dependent extraction refinement: turn classification, correction
/// detection, quality filters, and fact type classification.
pub mod refinement;
/// PR lesson extraction from training data (violations/lint JSONL) with quality gates.
pub mod training;

pub(crate) mod engine;
mod error;
mod provider;
mod types;
// WHY: `slugify` is the canonical entity-id scheme; the v17->v18 fact-entity
// backfill in `knowledge_store::migration` reuses it to infer edges, so the
// module is visible crate-wide (#4675).
pub(crate) mod utils;

pub use engine::ExtractionEngine;
pub use error::{ExtractionError, LlmCallSnafu, ParseResponseSnafu, PersistSnafu};
pub use eval::{ExtractionScores, LabeledFixture, score_extraction};
pub use provider::ExtractionProvider;
pub use types::{
    BookkeepingProviderKind, ConversationMessage, ExtractedEntity, ExtractedFact,
    ExtractedRelationship, ExtractedToolCall, Extraction, ExtractionConfig, ExtractionPrompt,
    PersistResult, RefinedExtraction,
};

#[cfg(test)]
mod tests;
