//! aletheia-melete — context distillation engine
//!
//! Melete (Μελέτη) — "practice, exercise." One of the three original Muses,
//! representing meditation and the discipline of focused thought. Compresses
//! conversation history into structured summaries that preserve essential
//! context across distillation boundaries.
//!
//! ## Role in the nous pipeline
//!
//! Melete is invoked by the nous (agent) layer when context window utilisation
//! exceeds a threshold. It calls the LLM to summarise older messages into a
//! structured [`distill::DistillResult`] and optionally writes high-value
//! items to persistent storage via [`flush::MemoryFlush`] before they are
//! evicted from the active context.
//!
//! ## Key types
//!
//! - [`distill::DistillEngine`] — runs distillation against an [`aletheia_hermeneus::provider::LlmProvider`]
//! - [`distill::DistillConfig`] / [`distill::DistillSection`] — configure model, token budget, and section layout
//! - [`flush::MemoryFlush`] / [`flush::FlushItem`] — pre-distillation memory flush payload
//!
//! Depends on `aletheia-koina` (types) and `aletheia-hermeneus` (LLM provider).

pub mod distill;
pub mod error;
pub mod flush;
pub mod prompt;
pub mod types;

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
