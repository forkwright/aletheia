//! Matrix channel provider (issue #3557).
//!
//! Phase 2 scaffold: compilable, feature-gated, with a real HTTP probe and a
//! fjall-backed [`CryptoStore`]. `send()` and `listen()` are stubs until
//! Phase 3 wires `matrix-sdk-base` sync.
//!
//! [`CryptoStore`]: matrix_sdk_crypto::store::CryptoStore

/// HTTP client wrapper around the Matrix homeserver (currently probe-only).
pub mod client;
/// Fjall-backed `CryptoStore` implementation (custom — avoids rusqlite).
pub mod crypto_store;
/// Matrix-specific error types.
pub mod error;
/// Serialisable snapshot of the crypto store (split for file-size bounds).
pub(crate) mod snapshot;
/// Placeholder sync loop (Phase 3 will populate it).
pub(crate) mod sync;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};

use crate::types::{
    ChannelCapabilities, ChannelProvider, InboundMessage, ProbeResult,
    SendParams as ChannelSendParams, SendResult,
};

pub use client::MatrixClient;
pub use crypto_store::FjallCryptoStore;

static MATRIX_CAPABILITIES: ChannelCapabilities = ChannelCapabilities {
    threads: true,
    reactions: true,
    typing: true,
    media: true,
    streaming: false,
    rich_formatting: true,
    // WHY: conduwuit accepts larger event bodies by default; 32 KiB is a
    // conservative per-message cap that fits well under the homeserver's
    // 20 MB request size while matching typical client UIs.
    max_text_length: 32_768,
};

/// Matrix channel provider implementing [`ChannelProvider`].
///
/// Phase 2 scope:
/// - `probe()` — real HTTP GET `/_matrix/client/versions`.
/// - `send()` — returns a "Phase 3" error (never dispatches).
/// - `listen()` — returns a closed receiver + empty `JoinSet`.
/// - Holds an `Arc<FjallCryptoStore>` so Phase 3 can plug in
///   `matrix-sdk-base` without reshaping the registration path.
pub struct MatrixProvider {
    homeserver_url: String,
    user_id: String,
    device_display_name: String,
    http: MatrixClient,
    crypto_store: Arc<FjallCryptoStore>,
    accounts: HashMap<String, AccountBinding>,
}

/// Per-agent binding: maps a `nous_id` to the Matrix room it sends/receives on.
#[derive(Debug, Clone)]
pub struct AccountBinding {
    /// Nous (agent) identifier.
    pub nous_id: String,
    /// Matrix room ID the agent operates in.
    pub room: String,
}

impl MatrixProvider {
    /// Construct a new provider with the supplied homeserver URL, identity,
    /// and fjall-backed crypto store.
    ///
    /// Phase 2 does not log in or start sync; it only validates the URL and
    /// builds the HTTP client. Phase 3 will connect via `matrix-sdk-base`.
    pub fn new(
        homeserver_url: impl Into<String>,
        user_id: impl Into<String>,
        device_display_name: impl Into<String>,
        crypto_store: Arc<FjallCryptoStore>,
    ) -> error::Result<Self> {
        let homeserver_url = homeserver_url.into();
        let http = MatrixClient::new(&homeserver_url)?;
        Ok(Self {
            homeserver_url,
            user_id: user_id.into(),
            device_display_name: device_display_name.into(),
            http,
            crypto_store,
            accounts: HashMap::new(),
        })
    }

    /// Register a `nous_id` → room mapping.
    pub fn add_account(&mut self, binding: AccountBinding) {
        self.accounts.insert(binding.nous_id.clone(), binding);
    }

    /// Matrix `user_id` this provider authenticates as (e.g. `@syn:menos.lan`).
    #[must_use]
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// Device display name advertised to the homeserver.
    #[must_use]
    pub fn device_display_name(&self) -> &str {
        &self.device_display_name
    }

    /// Homeserver base URL.
    #[must_use]
    pub fn homeserver_url(&self) -> &str {
        &self.homeserver_url
    }

    /// Expose the crypto store so operators (and Phase 3) can inspect or
    /// share it.
    #[must_use]
    pub fn crypto_store(&self) -> &Arc<FjallCryptoStore> {
        &self.crypto_store
    }

    /// Return the configured nous → room bindings (read-only).
    #[must_use]
    pub fn accounts(&self) -> &HashMap<String, AccountBinding> {
        &self.accounts
    }

    /// Start the (currently stub) inbound sync loop.
    ///
    /// Phase 2 returns an empty `JoinSet` and a closed receiver. Consumers
    /// can wire the provider into `ChannelListener` today; the receiver
    /// stays idle until Phase 3.
    ///
    /// `poll_interval` is accepted (matching [`SignalProvider::listen`]) but
    /// ignored until Phase 3 plugs in `matrix-sdk-base` sync — at which
    /// point it becomes the long-poll timeout for `/sync`.
    ///
    /// [`SignalProvider::listen`]: crate::semeion::SignalProvider::listen
    #[instrument(skip(self, cancel))]
    pub fn listen(
        &self,
        poll_interval: Option<Duration>,
        cancel: CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
        // Swallow intentionally-unused param without triggering clippy; the
        // parameter is a forward-compat hook for Phase 3.
        let _ = poll_interval;
        info!(user = %self.user_id, "matrix listen() invoked — sync loop lands in Phase 3");
        sync::start(cancel, 64)
    }
}

impl std::fmt::Debug for MatrixProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixProvider")
            .field("homeserver_url", &self.homeserver_url)
            .field("user_id", &self.user_id)
            .field("device_display_name", &self.device_display_name)
            .field("accounts", &self.accounts.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl ChannelProvider for MatrixProvider {
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn id(&self) -> &str {
        "matrix"
    }

    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "trait signature requires &str"
    )]
    fn name(&self) -> &str {
        "Matrix (conduwuit)"
    }

    fn capabilities(&self) -> &ChannelCapabilities {
        &MATRIX_CAPABILITIES
    }

    fn send<'a>(
        &'a self,
        _params: &'a ChannelSendParams,
    ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
        Box::pin(async move {
            SendResult::err("matrix: not yet wired (Phase 3)")
        })
    }

    fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
        Box::pin(async move {
            match self.http.versions().await {
                Ok(report) => {
                    let ok = report.status == 200;
                    ProbeResult {
                        ok,
                        latency_ms: Some(report.latency_ms),
                        error: if ok {
                            None
                        } else {
                            Some(format!("status {}", report.status))
                        },
                        details: Some(HashMap::from([
                            (
                                "homeserver".to_owned(),
                                serde_json::Value::String(self.homeserver_url.clone()),
                            ),
                            (
                                "status".to_owned(),
                                serde_json::Value::Number(report.status.into()),
                            ),
                        ])),
                    }
                }
                Err(e) => ProbeResult {
                    ok: false,
                    latency_ms: None,
                    error: Some(e.to_string()),
                    details: Some(HashMap::from([(
                        "homeserver".to_owned(),
                        serde_json::Value::String(self.homeserver_url.clone()),
                    )])),
                },
            }
        })
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use organon::testing::install_crypto_provider;

    use super::*;

    fn build_provider(url: &str) -> MatrixProvider {
        let store = Arc::new(FjallCryptoStore::open_temp("test-agent").expect("open temp store"));
        MatrixProvider::new(url, "@syn:menos.lan", "menos-syn", store).expect("provider")
    }

    #[tokio::test]
    async fn new_with_valid_config_succeeds() {
        let provider = build_provider("http://127.0.0.1:6167");
        assert_eq!(provider.user_id(), "@syn:menos.lan");
        assert_eq!(provider.device_display_name(), "menos-syn");
        assert_eq!(provider.homeserver_url(), "http://127.0.0.1:6167");
    }

    #[tokio::test]
    async fn new_rejects_invalid_url() {
        let store = Arc::new(FjallCryptoStore::open_temp("test-agent").expect("store"));
        let err = MatrixProvider::new("menos.lan:6167", "@syn:menos.lan", "menos-syn", store)
            .expect_err("should reject");
        assert!(format!("{err}").contains("http://"));
    }

    #[test]
    fn capabilities_shape() {
        assert!(MATRIX_CAPABILITIES.threads);
        assert!(MATRIX_CAPABILITIES.reactions);
        assert!(MATRIX_CAPABILITIES.rich_formatting);
        assert!(!MATRIX_CAPABILITIES.streaming);
        assert_eq!(MATRIX_CAPABILITIES.max_text_length, 32_768);
    }

    #[tokio::test]
    async fn probe_200_is_healthy() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/versions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"versions": ["v1.11"]})),
            )
            .mount(&server)
            .await;

        let provider = build_provider(&server.uri());
        let result = provider.probe().await;
        assert!(result.ok, "probe should be ok, got: {result:?}");
        assert!(result.latency_ms.is_some());
    }

    #[tokio::test]
    async fn probe_502_is_unhealthy() {
        install_crypto_provider();
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/versions"))
            .respond_with(wiremock::ResponseTemplate::new(502))
            .mount(&server)
            .await;

        let provider = build_provider(&server.uri());
        let result = provider.probe().await;
        assert!(!result.ok);
        assert!(result.error.as_deref().unwrap_or("").contains("502"));
    }

    #[tokio::test]
    async fn send_returns_phase3_stub() {
        let provider = build_provider("http://127.0.0.1:6167");
        let params = ChannelSendParams {
            to: "!room:menos.lan".to_owned(),
            text: "hello".to_owned(),
            account_id: None,
            thread_id: None,
            attachments: None,
        };
        let result = provider.send(&params).await;
        assert!(!result.sent);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("Phase 3")
        );
    }

    #[tokio::test]
    async fn listen_returns_empty_joinset_and_closed_rx() {
        let provider = build_provider("http://127.0.0.1:6167");
        let token = CancellationToken::new();
        let (mut rx, handles) = provider.listen(None, token.clone());
        assert!(handles.is_empty());
        assert!(rx.recv().await.is_none());
    }

    #[test]
    fn add_account_registers_binding() {
        let mut provider = build_provider("http://127.0.0.1:6167");
        provider.add_account(AccountBinding {
            nous_id: "syn".to_owned(),
            room: "!abcd:menos.lan".to_owned(),
        });
        assert_eq!(provider.accounts().len(), 1);
        assert!(provider.accounts().contains_key("syn"));
    }

    #[tokio::test]
    async fn id_and_name_match_spec() {
        let provider = build_provider("http://127.0.0.1:6167");
        assert_eq!(ChannelProvider::id(&provider), "matrix");
        assert_eq!(ChannelProvider::name(&provider), "Matrix (conduwuit)");
    }
}
