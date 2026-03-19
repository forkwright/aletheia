//! OpenAI-compatible local LLM provider for vLLM and similar servers.
//!
//! Routes requests with the `local/` model prefix to a configurable
//! OpenAI-compatible endpoint. Supports both streaming and non-streaming
//! completions, including function/tool calling.
//!
//! Gated behind the `local-llm` feature flag.

mod provider;
pub(crate) mod stream;
pub(crate) mod types;

pub use provider::{LocalProvider, LocalProviderConfig};
