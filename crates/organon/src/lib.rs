#![deny(missing_docs)]
//! aletheia-organon: tool registry, definitions, and built-in tool stubs
//!
//! Organon (ὄργανον): "instrument." The formal instruments through which
//! agent capability expresses. Provides the tool registry, definition types,
//! and stub implementations for built-in tools.
//!
//! Depends on `aletheia-koina` (types) and `aletheia-hermeneus` (LLM wire format).

/// Built-in tool implementations that ship with the platform.
pub mod builtins;
/// Organon-specific error types and result alias.
pub mod error;
/// Prometheus metrics for tool execution counts, latency, and error rates.
pub mod metrics;
/// RAII guard for subprocess lifecycle: kills and reaps on drop.
pub(crate) mod process_guard;
/// Central tool registry for runtime discovery and dispatch.
pub mod registry;
/// Landlock + seccomp sandbox for tool execution.
pub mod sandbox;
/// Tool definition, parameter schema, and executor trait.
pub mod types;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::registry::ToolRegistry;

    assert_impl_all!(ToolRegistry: Send, Sync);
}
