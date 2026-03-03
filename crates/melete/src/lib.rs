//! aletheia-melete — context distillation engine
//!
//! Melete (Μελέτη) — "practice, exercise." One of the three original Muses,
//! representing meditation and the discipline of focused thought. Compresses
//! conversation history into structured summaries that preserve essential
//! context across distillation boundaries.
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
