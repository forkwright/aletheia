//! Anthropic Messages API provider implementation.
//!
//! Includes the blocking HTTP client, SSE streaming parser, wire type mappings,
//! and error classification. Re-exports `AnthropicProvider` and `StreamEvent`
//! as the public surface.

pub mod batch;
mod client;
pub(crate) mod error;
pub(crate) mod stream;
pub(crate) mod wire;

pub use client::AnthropicProvider;
pub use stream::StreamEvent;
