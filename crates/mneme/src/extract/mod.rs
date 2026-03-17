//! Knowledge extraction pipeline: LLM-driven entity/relationship/fact extraction.

/// Context-dependent extraction refinement: turn classification, correction
/// detection, quality filters, and fact type classification.
pub mod refinement;

mod engine;
mod error;
mod provider;
mod types;
mod utils;

pub use engine::*;
pub use error::*;
pub use provider::*;
pub use types::*;

#[cfg(test)]
mod tests;
