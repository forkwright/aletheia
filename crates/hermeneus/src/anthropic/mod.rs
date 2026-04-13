//! Anthropic Messages API provider implementation.
//!
//! Includes the async HTTP client, SSE streaming parser, wire type mappings,
//! and error classification. Re-exports `AnthropicProvider` and `StreamEvent`
//! as the public surface.

pub(crate) mod batch;
pub(crate) mod cc_profile;
mod client;
pub(crate) mod error;
pub(crate) mod pricing;
pub(crate) mod stream;
pub(crate) mod wire;

pub use client::{AnthropicProvider, ProviderBehavior};
pub use stream::StreamEvent;
