#![deny(missing_docs)]
//! aletheia-melete: context distillation engine

/// Cross-chunk contradiction detection via LLM comparison.
pub mod contradiction;
/// Context distillation engine: token budgeting, LLM-driven summarization, and verbatim tail preservation.
pub mod distill;
/// Auto-dream memory consolidation: triple-gate background process for knowledge graph maintenance.
pub mod dream;
/// Melete-specific error types for distillation failures.
pub mod error;
/// Memory flush types for persisting critical context before distillation boundaries.
pub mod flush;
/// Prometheus metric definitions for distillation operations.
pub mod metrics;
/// Backward-path probe QA: verify distilled facts before committing to the knowledge graph.
pub mod probe;
/// System prompt construction and message formatting for the distillation LLM.
pub mod prompt;
/// Jaccard similarity pruning to remove near-duplicate content before distillation.
pub mod similarity;
/// Re-exported hermeneus types used by distillation consumers.
pub mod types;

#[cfg(test)]
mod roundtrip_tests;

#[cfg(test)]
mod assertions {
    use super::contradiction::{Contradiction, ContradictionLog};
    use super::distill::{DistillConfig, DistillEngine, DistillResult, DistillSection};
    use super::flush::{FlushItem, MemoryFlush};
    use super::similarity::PruningStats;

    const _: fn() = || {
        fn assert<T: Send + Sync>() {}
        assert::<DistillEngine>();
        assert::<DistillResult>();
        assert::<DistillConfig>();
        assert::<DistillSection>();
        assert::<MemoryFlush>();
        assert::<FlushItem>();
        assert::<PruningStats>();
        assert::<ContradictionLog>();
        assert::<Contradiction>();
    };
}
