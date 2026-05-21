//! Codex subprocess LLM provider.
//!
//! Delegates LLM calls to the `codex` CLI binary, which handles local
//! OAuth authentication. Gated behind the `codex-provider` feature flag.

mod parse;
mod process;
mod provider;

pub use provider::{CodexProvider, CodexProviderConfig};
