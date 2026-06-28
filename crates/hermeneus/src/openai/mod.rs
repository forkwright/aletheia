// WHY: `clippy::doc_markdown` would flag brand names in the rustdoc prose;
// the module-level allow keeps the docs readable without noisy backticks.
#![allow(clippy::doc_markdown)]

//! OpenAI LLM provider.
//!
//! Talks to the first-party OpenAI `/v1/responses` API or to endpoints
//! exposing the OpenAI `/v1/chat/completions` wire format — llama.cpp
//! `--server`, ollama, vllm, or a compatible proxy. Intended primary use:
//! local LLMs for air-gapped operation (#3414) and non-Anthropic cloud
//! alternatives (#3424).
//!
//! # Feature negotiation
//!
//! Maps Anthropic concepts onto OpenAI where possible and degrades
//! gracefully where not. See [`wire::request`] for the full mapping table.
//! Highlights:
//!
//! - `system` top-level prompt → first `{role: "system"}` message.
//! - `ContentBlock::ToolUse` ↔ assistant `tool_calls[]`.
//! - `ContentBlock::ToolResult` → `{role: "tool", tool_call_id: ...}`.
//! - `thinking` budget → dropped, warning logged.
//! - `cache_control` markers → dropped, warning logged.
//! - `server_tools` → rejected with a clear error.
//!
//! # Example
//!
//! ```no_run
//! use hermeneus::openai::{OpenAiProvider, OpenAiProviderConfig};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = OpenAiProvider::new(OpenAiProviderConfig {
//!     name: "local-qwen".to_owned(),
//!     base_url: "http://127.0.0.1:8088/v1".to_owned(),
//!     api_key: None,
//!     models: vec!["Qwen3.5-35B-A3B-Q8_0".to_owned()],
//!     ..OpenAiProviderConfig::default()
//! })?;
//! # let _ = provider;
//! # Ok(())
//! # }
//! ```

mod client;
mod error;
mod record;
mod wire;

pub use client::{OpenAiApiFamily, OpenAiProvider, OpenAiProviderConfig};

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices asserted valid by construction"
)]
mod tests;
