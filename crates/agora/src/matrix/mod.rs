//! Matrix channel provider: E2E encrypted messaging via matrix-rust-sdk.
//!
//! Connects to a self-hosted conduwuit homeserver for sovereign OOB
//! communications. Used for KAIROS heartbeat, operator escalation,
//! and nous-to-operator messaging.
//!
//! WHY Matrix over Signal: Matrix is self-hosted (conduwuit on menos),
//! pure Rust E2E crypto (vodozemac), no dependency on external servers.
//! Signal requires Signal Inc. servers for message routing.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::types::{
    ChannelCapabilities, ChannelProvider, ProbeResult, SendParams, SendResult,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the Matrix channel provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MatrixConfig {
    /// Homeserver URL (e.g., "https://menos.lan:6167").
    pub homeserver_url: String,
    /// Matrix user ID (e.g., "@kanon:menos.lan").
    pub user_id: String,
    /// Default room for escalation messages.
    pub escalation_room: Option<String>,
    /// Password for login (stored in credentials, not config).
    pub password: Option<String>,
    /// Path to the crypto store (E2E encryption keys).
    pub crypto_store_path: Option<String>,
    /// Enable E2E encryption (default: true).
    pub e2e_enabled: bool,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            homeserver_url: "http://127.0.0.1:6167".to_owned(),
            user_id: "@kanon:menos.lan".to_owned(),
            escalation_room: None,
            password: None,
            crypto_store_path: None,
            e2e_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix provider
// ---------------------------------------------------------------------------

/// Matrix channel provider using matrix-rust-sdk.
///
/// Currently a typed configuration holder and trait implementation stub.
/// Full SDK integration depends on adding matrix-sdk as a dependency
/// (separate PR after conduwuit is deployed and tested).
///
/// WHY stub first: the ChannelProvider trait registration, config loading,
/// and routing infrastructure can be wired before the SDK is integrated.
/// This lets us test the channel plumbing with a mock before adding the
/// matrix-sdk dependency (which pulls in ~50 transitive crates).
pub struct MatrixProvider {
    config: MatrixConfig,
}

impl MatrixProvider {
    /// Create a new Matrix provider from configuration.
    #[must_use]
    pub fn new(config: MatrixConfig) -> Self {
        info!(
            homeserver = %config.homeserver_url,
            user = %config.user_id,
            e2e = config.e2e_enabled,
            "matrix provider initialized"
        );
        Self { config }
    }

    /// Get the provider configuration.
    #[must_use]
    pub fn config(&self) -> &MatrixConfig {
        &self.config
    }
}

impl ChannelProvider for MatrixProvider {
    fn id(&self) -> &str {
        "matrix"
    }

    fn name(&self) -> &str {
        "Matrix (conduwuit)"
    }

    fn capabilities(&self) -> &ChannelCapabilities {
        // WHY: static ref — capabilities don't change at runtime.
        static CAPS: ChannelCapabilities = ChannelCapabilities {
            threads: true,
            reactions: true,
            typing: false,
            media: false, // WHY: text-only for heartbeat/escalation.
            streaming: false,
            rich_formatting: true, // Matrix supports markdown.
            max_text_length: 65536,
        };
        &CAPS
    }

    fn send<'a>(
        &'a self,
        params: &'a SendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
        Box::pin(async move {
            // TODO(#2602): replace with matrix-sdk Client::send_message()
            // once the dependency is added and conduwuit is deployed.
            warn!(
                to = %params.to,
                text_len = params.text.len(),
                "matrix send not yet implemented — SDK integration pending"
            );
            SendResult {
                sent: false,
                error: Some("matrix SDK not yet integrated".to_owned()),
            }
        })
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        Box::pin(async move {
            // TODO(#2602): replace with matrix-sdk health check
            // (client.homeserver_url() + /_matrix/client/versions)
            ProbeResult {
                ok: false,
                latency_ms: None,
                error: Some("matrix probe not yet implemented".to_owned()),
                details: None,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn provider_id_and_name() {
        let provider = MatrixProvider::new(MatrixConfig::default());
        assert_eq!(provider.id(), "matrix");
        assert_eq!(provider.name(), "Matrix (conduwuit)");
    }

    #[test]
    fn capabilities() {
        let provider = MatrixProvider::new(MatrixConfig::default());
        let caps = provider.capabilities();
        assert!(caps.threads);
        assert!(caps.rich_formatting);
        assert!(!caps.media);
    }

    #[test]
    fn config_roundtrip() {
        let config = MatrixConfig {
            homeserver_url: "https://test.lan:6167".to_owned(),
            user_id: "@test:test.lan".to_owned(),
            escalation_room: Some("!abc:test.lan".to_owned()),
            password: None,
            crypto_store_path: Some("/tmp/crypto".to_owned()),
            e2e_enabled: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MatrixConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.homeserver_url, "https://test.lan:6167");
        assert!(deserialized.e2e_enabled);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_returns_not_implemented() {
        let provider = MatrixProvider::new(MatrixConfig::default());
        let params = SendParams {
            to: "@operator:menos.lan".to_owned(),
            text: "heartbeat".to_owned(),
            account_id: None,
            thread_id: None,
            attachments: None,
        };
        let result = provider.send(&params).await;
        assert!(!result.sent);
        assert!(result.error.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_returns_not_implemented() {
        let provider = MatrixProvider::new(MatrixConfig::default());
        let result = provider.probe().await;
        assert!(!result.ok);
    }
}
