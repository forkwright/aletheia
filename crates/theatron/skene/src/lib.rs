#![deny(missing_docs)]
//! Shared API client, types, SSE, and streaming infrastructure for Aletheia UIs.
//!
//! This crate provides the protocol layer that both the TUI and desktop
//! frontends depend on: HTTP client, request/response types, SSE connection
//! management, per-turn streaming, and domain identifier newtypes.

/// HTTP client, SSE connection, and per-message streaming.
pub mod api;

/// Auto-discover a running aletheia server on the local network.
pub mod discovery;

/// Parsed streaming events from the per-session SSE endpoint.
pub mod events;

/// Newtype wrappers for domain identifiers shared across all frontends.
pub mod id;

/// Shared bearer-token secret storage core: OS keyring, AES-256-GCM
/// encrypted fallback, atomic secure writes.
pub mod secret_store;

/// SSE wire protocol parser for reqwest response streams.
pub mod sse;

/// Shared chat-transcript text projections used by both first-party frontends.
pub mod text;

/// Install the rustls crypto provider for tests that build reqwest clients.
/// Production installs it at startup; tests must install explicitly.
#[cfg(test)]
pub(crate) fn install_test_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    // kanon:ignore RUST/allow-not-expect — test module needs use super::* for kanon test-missing-use-super rule; tests use fully-qualified super:: paths
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn public_modules_exist() {
        // WHY: smoke test verifying the public modules compile and link
        let _ = std::any::type_name::<super::api::ApiClient>();
        let _ = std::any::type_name_of_val(&super::discovery::discover_server);
        let _ = std::any::type_name::<super::id::ApiNousId>();
        let _ = std::any::type_name::<super::secret_store::TokenStore>();
        let _ = std::any::type_name_of_val(&super::text::append_terminal_notice);
    }
}
