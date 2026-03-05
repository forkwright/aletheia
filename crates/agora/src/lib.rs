//! aletheia-agora — channel registry and provider implementations
//!
//! Agora (ἀγορά) — "gathering place." The public square where messages flow
//! between Aletheia and the outside world. Provides the channel abstraction
//! and registry, with Signal (semeion) as the first provider.

/// Error types for channel operations and provider failures.
pub mod error;
/// Unified channel listener that merges inbound messages from all providers into a single stream.
pub mod listener;
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
    use static_assertions::assert_impl_all;

    use super::listener::ChannelListener;
    use super::registry::ChannelRegistry;
    use super::router::MessageRouter;
    use super::semeion::SignalProvider;
    use super::semeion::client::SignalClient;
    use super::types::InboundMessage;

    assert_impl_all!(ChannelRegistry: Send, Sync);
    assert_impl_all!(ChannelListener: Send);
    assert_impl_all!(InboundMessage: Send, Sync);
    assert_impl_all!(MessageRouter: Send, Sync);
    assert_impl_all!(SignalClient: Send, Sync);
    assert_impl_all!(SignalProvider: Send, Sync);
}
