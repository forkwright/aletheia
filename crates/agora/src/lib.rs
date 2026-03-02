//! aletheia-agora — channel registry and provider implementations
//!
//! Agora (ἀγορά) — "gathering place." The public square where messages flow
//! between Aletheia and the outside world. Provides the channel abstraction
//! and registry, with Signal (semeion) as the first provider.

pub mod error;
pub mod listener;
pub mod registry;
pub mod semeion;
pub mod types;

#[cfg(test)]
mod assertions {
    use static_assertions::assert_impl_all;

    use super::listener::ChannelListener;
    use super::registry::ChannelRegistry;
    use super::semeion::client::SignalClient;
    use super::semeion::SignalProvider;
    use super::types::InboundMessage;

    assert_impl_all!(ChannelRegistry: Send, Sync);
    assert_impl_all!(ChannelListener: Send);
    assert_impl_all!(InboundMessage: Send, Sync);
    assert_impl_all!(SignalClient: Send, Sync);
    assert_impl_all!(SignalProvider: Send, Sync);
}
