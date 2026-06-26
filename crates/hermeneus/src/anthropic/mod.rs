//! Anthropic Messages API provider implementation.
//!
//! Includes the async HTTP client, SSE streaming parser, wire type mappings,
//! and error classification. Re-exports `AnthropicProvider`, `StreamEvent`,
//! and the default behavior constants as the public surface.

pub(crate) mod cc_profile;
mod client;
pub(crate) mod error;
pub(crate) mod pricing;
pub(crate) mod stream;
pub(crate) mod wire;

pub use client::{AnthropicProvider, NON_STREAMING_TIMEOUT, ProviderBehavior};
pub use error::SSE_DEFAULT_RETRY_MS;
pub use stream::StreamEvent;
