#![deny(missing_docs)]
//! aletheia-agora: channel registry and provider implementations
//!
//! Agora (ἀγορά): "gathering place." The public square where messages flow
//! between Aletheia and the outside world. Provides the channel abstraction
//! and registry, with Signal (semeion) as the first provider.

/// Error types for channel operations and provider failures.
pub(crate) mod error;
/// Unified channel listener that merges inbound messages from all providers into a single stream.
pub mod listener;
/// Prometheus metric definitions for channel messaging.
pub mod metrics;
/// Channel registry: the single source of truth for available channel providers.
pub mod registry;
/// Message routing: resolves inbound messages to the appropriate nous agent.
pub mod router;
/// Signal channel provider backed by the signal-cli JSON-RPC daemon.
pub mod semeion;
/// Core types for the channel abstraction layer (capabilities, send/receive, provider trait).
pub mod types;

#[cfg(test)]
mod assertions {
    use super::listener::ChannelListener;
    use super::registry::ChannelRegistry;
    use super::router::MessageRouter;
    use super::semeion::SignalProvider;
    use super::semeion::client::SignalClient;
    use super::types::InboundMessage;

    const _: fn() = || {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ChannelRegistry>();
        assert_send::<ChannelListener>();
        assert_send_sync::<InboundMessage>();
        assert_send_sync::<MessageRouter>();
        assert_send_sync::<SignalClient>();
        assert_send_sync::<SignalProvider>();
    };
}
