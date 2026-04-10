//! Re-exports API infrastructure from skene.
//!
//! All types, the HTTP client, SSE connection management, and streaming
//! live in `skene::api`. This module provides crate-local access
//! at the same paths the TUI used before the extraction.

/// HTTP client for the Aletheia gateway REST API.
pub mod client {
    pub use skene::api::client::*;
}

#[expect(
    unused_imports,
    reason = "re-exported so callers can name the error type via crate::api::ApiError"
)]
/// API error types.
pub mod error {
    pub use skene::api::error::*;
}

/// SSE connection management.
pub mod sse {
    pub use skene::api::sse::*;
}

#[expect(
    unused_imports,
    reason = "re-exported for callers that reference crate::api::streaming"
)]
/// Per-turn streaming.
pub mod streaming {
    pub use skene::api::streaming::*;
}

/// Request and response types.
pub mod types {
    pub use skene::api::types::*;
}

#[expect(
    unused_imports,
    reason = "re-exports for crate-level access; modules import submodules directly"
)]
pub use skene::api::{ApiClient, ApiError};
