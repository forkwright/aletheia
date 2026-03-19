//! Knowledge extraction pipeline: LLM-driven entity/relationship/fact extraction.

/// Unified diff parser for structured change analysis.
pub mod diff;
/// Post-merge lesson extraction from PR diffs.
pub mod lesson;
/// Context-dependent extraction refinement: turn classification, correction
/// detection, quality filters, and fact type classification.
pub mod refinement;

mod engine;
mod error;
mod provider;
mod types;
mod utils;

pub use engine::ExtractionEngine;
pub use error::{ExtractionError, LlmCallSnafu, ParseResponseSnafu, PersistSnafu};
pub use provider::ExtractionProvider;
pub use types::{
    ConversationMessage, ExtractedEntity, ExtractedFact, ExtractedRelationship, Extraction,
    ExtractionConfig, ExtractionPrompt, PersistResult, RefinedExtraction,
};

#[cfg(test)]
mod tests;
