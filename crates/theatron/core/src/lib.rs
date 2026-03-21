#![deny(missing_docs)]
//! Shared API client, types, SSE, and streaming infrastructure for Aletheia UIs.
//!
//! This crate provides the protocol layer that both the TUI and desktop
//! frontends depend on: HTTP client, request/response types, SSE connection
//! management, per-turn streaming, and domain identifier newtypes.

/// HTTP client, SSE connection, and per-message streaming.
pub mod api;

/// Parsed streaming events from the per-session SSE endpoint.
pub mod events;

/// Newtype wrappers for domain identifiers shared across all frontends.
pub mod id;

/// SSE wire protocol parser for reqwest response streams.
pub mod sse;

#[cfg(test)]
mod tests {
    #[test]
    fn public_modules_exist() {
        // WHY: smoke test verifying the four public modules compile and link
        let _ = std::any::type_name::<super::api::ApiClient>();
        let _ = std::any::type_name::<super::id::NousId>();
    }
}
