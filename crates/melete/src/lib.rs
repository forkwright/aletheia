#![deny(missing_docs)]
//! aletheia-melete: context distillation engine

/// Cross-chunk contradiction detection via LLM comparison.
pub mod contradiction;
/// Context distillation engine: token budgeting, LLM-driven summarization, and verbatim tail preservation.
pub mod distill;
/// Melete-specific error types for distillation failures.
pub mod error;
/// Prometheus metric definitions for distillation operations.
pub mod metrics;
/// Memory flush types for persisting critical context before distillation boundaries.
pub mod flush;
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
    use static_assertions::assert_impl_all;

    use super::contradiction::{Contradiction, ContradictionLog};
    use super::distill::{DistillConfig, DistillEngine, DistillResult, DistillSection};
    use super::flush::{FlushItem, MemoryFlush};
    use super::similarity::PruningStats;

    assert_impl_all!(DistillEngine: Send, Sync);
    assert_impl_all!(DistillResult: Send, Sync);
    assert_impl_all!(DistillConfig: Send, Sync);
    assert_impl_all!(DistillSection: Send, Sync);
    assert_impl_all!(MemoryFlush: Send, Sync);
    assert_impl_all!(FlushItem: Send, Sync);
    assert_impl_all!(PruningStats: Send, Sync);
    assert_impl_all!(ContradictionLog: Send, Sync);
    assert_impl_all!(Contradiction: Send, Sync);
}
