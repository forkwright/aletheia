//! aletheia-organon — tool registry, definitions, and built-in tool stubs
//!
//! Organon (ὄργανον) — "instrument." The formal instruments through which
//! agent capability expresses. Provides the tool registry, definition types,
//! and built-in tool implementations.
//!
//! ## Key types
//!
//! - [`registry::ToolRegistry`] — single source of truth for available tools;
//!   register via [`register`](registry::ToolRegistry::register), dispatch via
//!   [`execute`](registry::ToolRegistry::execute)
//! - [`registry::ToolExecutor`] — trait that tool implementations must satisfy
//! - [`types::ToolDef`] / [`types::ToolInput`] / [`types::ToolResult`] — tool lifecycle types
//! - [`types::ToolCategory`] / [`types::PropertyType`] — enum classifiers
//! - [`builtins::register_all`] — register all built-in tools in one call
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
