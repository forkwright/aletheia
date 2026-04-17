//! Placeholder sync loop for the Matrix channel provider.
//!
//! Phase 2: exposes only the type-level scaffolding the provider's `listen()`
//! returns. Phase 3 replaces this with a real /sync loop that:
//!   - drives `matrix-sdk-base` state deltas
//!   - decrypts inbound room events via the fjall-backed `CryptoStore`
//!   - maps Matrix events onto `crate::types::InboundMessage`
//!   - handles reconnect with agora's existing circuit-breaker pattern
//!
//! Keeping the module in place (with a documented stub) lets the provider
//! wire `JoinSet`s / `mpsc` channels up in Phase 2 without ceremony when
//! Phase 3 lands.

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::types::InboundMessage;

/// Spawn the (currently empty) Matrix sync task set.
///
/// Phase 2: returns an empty `JoinSet` and a closed receiver so callers can
/// wire the listener plumbing without feature-gating at every call site.
/// Phase 3 will add real sync tasks to the returned `JoinSet`.
pub(super) fn start(
    _cancel: CancellationToken,
    buffer_capacity: usize,
) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
    let (tx, rx) = mpsc::channel(buffer_capacity.max(1));
    // Drop the sender immediately so the receiver is closed — no spurious
    // inbound messages reach the channel listener until Phase 3.
    drop(tx);
    (rx, JoinSet::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_returns_closed_receiver_and_empty_joinset() {
        let cancel = CancellationToken::new();
        let (mut rx, handles) = start(cancel, 16);
        assert!(handles.is_empty());
        // Receiver should be immediately closed.
        assert!(rx.recv().await.is_none());
    }
}
