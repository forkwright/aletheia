//! aletheia-organon — tool registry, definitions, and built-in tool stubs
//!
//! Organon (ὄργανον) — "instrument." The formal instruments through which
//! agent capability expresses. Provides the tool registry, definition types,
//! and stub implementations for built-in tools.
//!
//! Depends on `aletheia-koina` (types) and `aletheia-hermeneus` (LLM wire format).

pub mod builtins;
pub mod error;
pub mod registry;
pub mod types;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::registry::ToolRegistry;

    assert_impl_all!(ToolRegistry: Send, Sync);
}
