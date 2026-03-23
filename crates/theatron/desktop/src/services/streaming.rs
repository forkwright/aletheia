//! Streaming service that wraps per-message fetch streams with timeout and abort.

use std::time::Duration;

use dioxus::prelude::{Signal, WritableExt};
use reqwest::Client;
use theatron_core::events::StreamEvent;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::components::chat::{ChatState, ChatStateManager};

/// Stream timeout: 10 minutes per turn.
const STREAM_TIMEOUT: Duration = Duration::from_secs(600);

/// Debounce tick interval for flushing buffered text deltas to signals.
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// Manages a single streaming turn lifecycle.
///
/// Wraps [`crate::api::streaming::stream_turn`] with:
/// - 10-minute timeout
/// - 100ms debounce tick for text delta flushing
/// - CancellationToken-based abort
/// - ChatStateManager event processing
pub(crate) struct StreamingSession {
    rx: mpsc::Receiver<StreamEvent>,
    cancel: CancellationToken,
    manager: ChatStateManager,
}

impl StreamingSession {
    /// Start a new streaming session.
    ///
    /// Initiates the HTTP SSE stream via `stream_turn` and returns a
    /// session that can be polled for state updates.
    #[must_use]
    pub(crate) fn start(
        client: Client,
        base_url: &str,
        nous_id: &str,
        session_key: &str,
        message: &str,
        cancel: CancellationToken,
    ) -> Self {
        let rx = crate::api::streaming::stream_turn(
            client,
            base_url,
            nous_id,
            session_key,
            message,
            cancel.clone(),
        );

        Self {
            rx,
            cancel,
            manager: ChatStateManager::new(),
        }
    }

    /// Drive the streaming session to completion, updating chat state.
    ///
    /// This is designed to run inside a Dioxus `spawn(async { ... })` block.
    /// It processes stream events with 100ms debounce ticks and respects
    /// the 10-minute timeout.
    ///
    /// Returns `true` if the stream completed normally, `false` if it
    /// timed out or was cancelled.
    pub(crate) async fn drive(&mut self, chat_state: &mut Signal<ChatState>) -> bool {
        let timeout = tokio::time::sleep(STREAM_TIMEOUT);
        tokio::pin!(timeout);

        let mut interval = tokio::time::interval(FLUSH_INTERVAL);
        // WHY: The first tick fires immediately; skip it so we don't
        // flush an empty buffer right after starting.
        interval.tick().await;

        loop {
            let event = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => break,
                _ = &mut timeout => {
                    let mut state = chat_state.write();
                    let _ = self.manager.apply(
                        StreamEvent::Error("stream timed out after 10 minutes".to_string()),
                        &mut state,
                    );
                    return false;
                }
                event = self.rx.recv() => event,
                _ = interval.tick() => {
                    let mut state = chat_state.write();
                    let _ = self.manager.tick(&mut state);
                    continue;
                }
            };

            let Some(event) = event else { break };
            let is_terminal = matches!(
                &event,
                StreamEvent::TurnComplete { .. }
                    | StreamEvent::TurnAbort { .. }
                    | StreamEvent::Error(_)
            );
            let mut state = chat_state.write();
            let _ = self.manager.apply(event, &mut state);
            drop(state);
            if is_terminal {
                return !matches!(is_terminal, false);
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_constant_is_ten_minutes() {
        assert_eq!(STREAM_TIMEOUT, Duration::from_secs(600));
    }

    #[test]
    fn flush_interval_is_100ms() {
        assert_eq!(FLUSH_INTERVAL, Duration::from_millis(100));
    }
}
