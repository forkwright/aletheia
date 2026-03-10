//! aletheia-melete — context distillation engine

/// Context distillation engine: token budgeting, LLM-driven summarization, and verbatim tail preservation.
pub mod distill;
/// Errors that can occur during distillation operations.
pub mod error;
/// Memory flush types for persisting critical context before distillation boundaries.
pub mod flush;
/// System prompt construction and message formatting for the distillation LLM.
pub mod prompt;
/// Re-exported hermeneus types used by distillation consumers.
pub mod types;

#[cfg(test)]
mod roundtrip_tests;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::distill::{DistillConfig, DistillEngine, DistillResult, DistillSection};
    use super::flush::{FlushItem, MemoryFlush};

    assert_impl_all!(DistillEngine: Send, Sync);
    assert_impl_all!(DistillResult: Send, Sync);
    assert_impl_all!(DistillConfig: Send, Sync);
    assert_impl_all!(DistillSection: Send, Sync);
    assert_impl_all!(MemoryFlush: Send, Sync);
    assert_impl_all!(FlushItem: Send, Sync);
}
