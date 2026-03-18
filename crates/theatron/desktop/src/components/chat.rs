//! Streaming chat component state machine.
//!
//! This module implements the state management logic for a streaming chat
//! component in Dioxus. It processes `StreamEvent`s into renderable state,
//! with 100ms debounce for text deltas to avoid excessive re-renders.
//!
//! # Architecture
//!
//! ```text
//!   ┌─────────────┐     stream_turn()     ┌──────────────┐
//!   │  User Input  │ ───────────────────►  │  SSE Stream   │
//!   │  Component   │                       │  (per-turn)   │
//!   └─────────────┘                       └──────┬───────┘
//!                                                │
//!                                    StreamEvent  │
//!                                                ▼
//!   ┌─────────────┐   100ms debounce   ┌──────────────┐
//!   │   Rendered   │ ◄──────────────── │  ChatState    │
//!   │   Output     │                   │  (signals)    │
//!   └─────────────┘                    └──────────────┘
//! ```
//!
//! # Signal-based state
//!
//! In Dioxus, each piece of mutable UI state is a `Signal<T>`. When
//! written, dependents re-render. The `ChatState` below models what
//! signals would hold; in production, each field becomes a `Signal<T>`
//! inside a Dioxus component scope.
//!
//! # Debounce strategy
//!
//! Text deltas arrive at high frequency (every few tokens). Writing to
//! the signal on every delta causes excessive re-renders. Instead:
//!
//! 1. Accumulate deltas into a local buffer.
//! 2. Flush to the signal when 100ms elapses or a non-delta event arrives.
//! 3. Also flush on newlines (users notice line breaks immediately).
//!
//! This mirrors the TUI's 64-byte/newline markdown cache invalidation
//! but is tuned for Dioxus's virtual DOM diffing cost.

use std::time::{Duration, Instant};

use crate::api::types::{ConnectionState, NousId, StreamEvent, StreamingState, ToolCallInfo};

/// How long to buffer text deltas before flushing to the signal.
const TEXT_DEBOUNCE: Duration = Duration::from_millis(100);

/// A committed chat message in the conversation history.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub model: Option<String>,
    pub tool_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Who produced a chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageRole {
    User,
    Assistant,
}

/// Complete chat state that drives the Dioxus component tree.
///
/// In a Dioxus component, each field here becomes a `Signal<T>`:
///
/// ```text
/// let messages = use_signal(Vec::new);
/// let streaming = use_signal(StreamingState::default);
/// let connection = use_signal(|| ConnectionState::Disconnected);
/// ```
///
/// The `ChatStateManager` below manages transitions; the signals
/// propagate changes to the rendered output.
#[derive(Debug, Clone)]
pub struct ChatState {
    /// Committed conversation history.
    pub messages: Vec<ChatMessage>,
    /// In-flight streaming state for the active turn.
    pub streaming: StreamingState,
    /// Global SSE connection state.
    pub connection: ConnectionState,
    /// Agent currently being chatted with.
    pub agent_id: Option<NousId>,
    /// Active session key.
    pub session_key: Option<String>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            streaming: StreamingState::default(),
            connection: ConnectionState::Disconnected,
            agent_id: None,
            session_key: None,
        }
    }
}

/// Manages `ChatState` transitions in response to `StreamEvent`s.
///
/// Encapsulates the debounce buffer and flush logic. In a Dioxus component,
/// this runs inside a `use_coroutine` that reads from the stream receiver
/// and writes into signals.
pub struct ChatStateManager {
    /// Pending text delta buffer: not yet flushed to state.
    text_buffer: String,
    /// Pending thinking delta buffer.
    thinking_buffer: String,
    /// Last time the text buffer was flushed.
    last_flush: Instant,
}

impl Default for ChatStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatStateManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            text_buffer: String::new(),
            thinking_buffer: String::new(),
            last_flush: Instant::now(),
        }
    }

    /// Process a stream event and apply it to the chat state.
    ///
    /// Returns `true` if the state was modified in a way that should
    /// trigger a re-render (i.e., the signal should be written to).
    /// Returns `false` if the delta was buffered and no flush is needed yet.
    #[must_use]
    pub fn apply(&mut self, event: StreamEvent, state: &mut ChatState) -> bool {
        match event {
            StreamEvent::TurnStart {
                turn_id,
                session_id: _,
                nous_id: _,
            } => {
                state.streaming = StreamingState {
                    text: String::new(),
                    thinking: String::new(),
                    tool_calls: Vec::new(),
                    is_streaming: true,
                    turn_id: Some(turn_id),
                    error: None,
                };
                self.text_buffer.clear();
                self.thinking_buffer.clear();
                self.last_flush = Instant::now();
                true
            }
            StreamEvent::TextDelta(delta) => {
                let has_newline = delta.contains('\n');
                self.text_buffer.push_str(&delta);
                self.maybe_flush_text(state, has_newline)
            }
            StreamEvent::ThinkingDelta(delta) => {
                let has_newline = delta.contains('\n');
                self.thinking_buffer.push_str(&delta);
                self.maybe_flush_thinking(state, has_newline)
            }
            StreamEvent::ToolStart { tool_name, tool_id } => {
                // Flush any pending text before recording tool start.
                self.flush_text(state);
                self.flush_thinking(state);
                state.streaming.tool_calls.push(ToolCallInfo {
                    tool_name,
                    tool_id,
                    is_error: false,
                    duration_ms: None,
                    completed: false,
                });
                true
            }
            StreamEvent::ToolResult {
                tool_id,
                is_error,
                duration_ms,
                ..
            } => {
                if let Some(tc) = state
                    .streaming
                    .tool_calls
                    .iter_mut()
                    .find(|tc| tc.tool_id == tool_id)
                {
                    tc.is_error = is_error;
                    tc.duration_ms = Some(duration_ms);
                    tc.completed = true;
                }
                true
            }
            StreamEvent::TurnComplete { outcome } => {
                // Flush remaining buffered text.
                self.flush_text(state);
                self.flush_thinking(state);

                let message = ChatMessage {
                    role: MessageRole::Assistant,
                    content: std::mem::take(&mut state.streaming.text),
                    model: Some(outcome.model),
                    tool_calls: outcome.tool_calls,
                    input_tokens: outcome.input_tokens,
                    output_tokens: outcome.output_tokens,
                };
                state.messages.push(message);
                state.streaming = StreamingState::default();
                true
            }
            StreamEvent::TurnAbort { reason } => {
                self.flush_text(state);
                tracing::info!(reason, "turn aborted");
                // Preserve partial text in history if any was generated.
                if !state.streaming.text.is_empty() {
                    let message = ChatMessage {
                        role: MessageRole::Assistant,
                        content: std::mem::take(&mut state.streaming.text),
                        model: None,
                        tool_calls: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                    };
                    state.messages.push(message);
                }
                state.streaming = StreamingState::default();
                true
            }
            StreamEvent::Error(msg) => {
                self.flush_text(state);
                state.streaming.error = Some(msg);
                state.streaming.is_streaming = false;
                true
            }
            StreamEvent::ToolApprovalRequired { .. } | StreamEvent::ToolApprovalResolved { .. } => {
                // These would drive overlay/dialog signals in the full implementation.
                true
            }
        }
    }

    /// Flush buffered text if the debounce interval has elapsed or a
    /// newline was received.
    #[must_use]
    fn maybe_flush_text(&mut self, state: &mut ChatState, has_newline: bool) -> bool {
        if has_newline || self.last_flush.elapsed() >= TEXT_DEBOUNCE {
            self.flush_text(state);
            return true;
        }
        false
    }

    /// Flush buffered thinking text with the same debounce logic.
    #[must_use]
    fn maybe_flush_thinking(&mut self, state: &mut ChatState, has_newline: bool) -> bool {
        if has_newline || self.last_flush.elapsed() >= TEXT_DEBOUNCE {
            self.flush_thinking(state);
            return true;
        }
        false
    }

    /// Unconditionally move buffered text into state.
    fn flush_text(&mut self, state: &mut ChatState) {
        if !self.text_buffer.is_empty() {
            state.streaming.text.push_str(&self.text_buffer);
            self.text_buffer.clear();
            self.last_flush = Instant::now();
        }
    }

    /// Unconditionally move buffered thinking text into state.
    fn flush_thinking(&mut self, state: &mut ChatState) {
        if !self.thinking_buffer.is_empty() {
            state.streaming.thinking.push_str(&self.thinking_buffer);
            self.thinking_buffer.clear();
        }
    }

    /// Force-flush all buffered text. Call this on a timer tick (100ms)
    /// from the Dioxus coroutine to ensure text is never stuck in the
    /// buffer longer than the debounce interval.
    #[must_use]
    pub fn tick(&mut self, state: &mut ChatState) -> bool {
        let mut changed = false;
        if !self.text_buffer.is_empty() {
            self.flush_text(state);
            changed = true;
        }
        if !self.thinking_buffer.is_empty() {
            self.flush_thinking(state);
            changed = true;
        }
        changed
    }
}

/// Apply a connection state change from the global SSE stream.
pub fn apply_connection_event(state: &mut ChatState, connected: bool) {
    state.connection = if connected {
        ConnectionState::Connected
    } else {
        match &state.connection {
            ConnectionState::Reconnecting { attempt } => ConnectionState::Reconnecting {
                attempt: attempt + 1,
            },
            _ => ConnectionState::Reconnecting { attempt: 1 },
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{ToolId, TurnOutcome};

    fn make_state() -> ChatState {
        ChatState::default()
    }

    fn make_manager() -> ChatStateManager {
        ChatStateManager::new()
    }

    #[test]
    fn turn_start_resets_streaming_state() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let changed = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        assert!(changed);
        assert!(state.streaming.is_streaming);
        assert!(state.streaming.text.is_empty());
        assert_eq!(state.streaming.turn_id.as_deref(), Some("t1"));
    }

    #[test]
    fn text_delta_with_newline_flushes_immediately() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        let changed = mgr.apply(StreamEvent::TextDelta("hello\n".to_string()), &mut state);
        assert!(changed);
        assert_eq!(state.streaming.text, "hello\n");
    }

    #[test]
    fn text_delta_without_newline_buffers() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        // Force the last_flush to be recent so debounce holds.
        mgr.last_flush = Instant::now();

        let changed = mgr.apply(StreamEvent::TextDelta("he".to_string()), &mut state);
        // Debounce not elapsed and no newline: should buffer.
        assert!(!changed);
        assert!(state.streaming.text.is_empty());

        // Tick flushes the buffer.
        let flushed = mgr.tick(&mut state);
        assert!(flushed);
        assert_eq!(state.streaming.text, "he");
    }

    #[test]
    fn text_delta_flushes_after_debounce_interval() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        // Set last_flush to 200ms ago so debounce has passed.
        mgr.last_flush = Instant::now() - Duration::from_millis(200);

        let changed = mgr.apply(StreamEvent::TextDelta("world".to_string()), &mut state);
        assert!(changed);
        assert_eq!(state.streaming.text, "world");
    }

    #[test]
    fn tool_start_flushes_pending_text() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        // Buffer some text (force recent flush so it doesn't auto-flush).
        mgr.last_flush = Instant::now();
        let _ = mgr.apply(StreamEvent::TextDelta("partial".to_string()), &mut state);
        assert!(state.streaming.text.is_empty());

        // Tool start forces flush.
        let _ = mgr.apply(
            StreamEvent::ToolStart {
                tool_name: "read_file".to_string(),
                tool_id: ToolId::from("t1"),
            },
            &mut state,
        );
        assert_eq!(state.streaming.text, "partial");
        assert_eq!(state.streaming.tool_calls.len(), 1);
        assert_eq!(state.streaming.tool_calls[0].tool_name, "read_file");
    }

    #[test]
    fn tool_result_updates_existing_tool_call() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        let _ = mgr.apply(
            StreamEvent::ToolStart {
                tool_name: "exec".to_string(),
                tool_id: ToolId::from("tool-1"),
            },
            &mut state,
        );
        let _ = mgr.apply(
            StreamEvent::ToolResult {
                tool_name: "exec".to_string(),
                tool_id: ToolId::from("tool-1"),
                is_error: false,
                duration_ms: 250,
            },
            &mut state,
        );

        assert!(state.streaming.tool_calls[0].completed);
        assert_eq!(state.streaming.tool_calls[0].duration_ms, Some(250));
    }

    #[test]
    fn turn_complete_commits_message_to_history() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        let _ = mgr.apply(StreamEvent::TextDelta("hello\n".to_string()), &mut state);

        let _ = mgr.apply(
            StreamEvent::TurnComplete {
                outcome: TurnOutcome {
                    text: "hello".to_string(),
                    nous_id: NousId::from("syn"),
                    session_id: "s1".into(),
                    model: "claude-opus-4-6".to_string(),
                    tool_calls: 0,
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    error: None,
                },
            },
            &mut state,
        );

        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].role, MessageRole::Assistant);
        assert_eq!(state.messages[0].content, "hello\n");
        assert!(!state.streaming.is_streaming);
    }

    #[test]
    fn turn_abort_preserves_partial_text() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        let _ = mgr.apply(StreamEvent::TextDelta("partial\n".to_string()), &mut state);

        let _ = mgr.apply(
            StreamEvent::TurnAbort {
                reason: "cancelled".to_string(),
            },
            &mut state,
        );

        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "partial\n");
        assert!(!state.streaming.is_streaming);
    }

    #[test]
    fn turn_abort_with_no_text_adds_no_message() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        let _ = mgr.apply(
            StreamEvent::TurnAbort {
                reason: "cancelled".to_string(),
            },
            &mut state,
        );

        assert!(state.messages.is_empty());
    }

    #[test]
    fn error_sets_error_and_stops_streaming() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        let _ = mgr.apply(
            StreamEvent::Error("connection lost".to_string()),
            &mut state,
        );

        assert_eq!(state.streaming.error.as_deref(), Some("connection lost"));
        assert!(!state.streaming.is_streaming);
    }

    #[test]
    fn apply_connection_event_connected() {
        let mut state = make_state();
        apply_connection_event(&mut state, true);
        assert_eq!(state.connection, ConnectionState::Connected);
    }

    #[test]
    fn apply_connection_event_disconnect_increments_attempt() {
        let mut state = make_state();
        apply_connection_event(&mut state, false);
        assert_eq!(
            state.connection,
            ConnectionState::Reconnecting { attempt: 1 }
        );
        apply_connection_event(&mut state, false);
        assert_eq!(
            state.connection,
            ConnectionState::Reconnecting { attempt: 2 }
        );
    }

    #[test]
    fn apply_connection_event_reconnect_resets_on_connect() {
        let mut state = make_state();
        apply_connection_event(&mut state, false);
        apply_connection_event(&mut state, false);
        apply_connection_event(&mut state, true);
        assert_eq!(state.connection, ConnectionState::Connected);
    }

    #[test]
    fn thinking_delta_flushes_on_newline() {
        let mut state = make_state();
        let mut mgr = make_manager();

        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );

        let changed = mgr.apply(
            StreamEvent::ThinkingDelta("step 1\n".to_string()),
            &mut state,
        );
        assert!(changed);
        assert_eq!(state.streaming.thinking, "step 1\n");
    }

    #[test]
    fn tick_returns_false_when_nothing_buffered() {
        let mut state = make_state();
        let mut mgr = make_manager();
        assert!(!mgr.tick(&mut state));
    }

    #[test]
    fn chat_state_default() {
        let state = ChatState::default();
        assert!(state.messages.is_empty());
        assert!(!state.streaming.is_streaming);
        assert_eq!(state.connection, ConnectionState::Disconnected);
        assert!(state.agent_id.is_none());
    }

    #[test]
    fn full_turn_lifecycle() {
        let mut state = make_state();
        let mut mgr = make_manager();

        // User sends a message.
        state.messages.push(ChatMessage {
            role: MessageRole::User,
            content: "Hello".to_string(),
            model: None,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
        });

        // Turn starts.
        let _ = mgr.apply(
            StreamEvent::TurnStart {
                session_id: "s1".into(),
                nous_id: "syn".into(),
                turn_id: "t1".into(),
            },
            &mut state,
        );
        assert!(state.streaming.is_streaming);

        // Text arrives.
        let _ = mgr.apply(StreamEvent::TextDelta("Hi ".to_string()), &mut state);
        let _ = mgr.apply(StreamEvent::TextDelta("there!\n".to_string()), &mut state);
        assert_eq!(state.streaming.text, "Hi there!\n");

        // Tool call.
        let _ = mgr.apply(
            StreamEvent::ToolStart {
                tool_name: "search".to_string(),
                tool_id: ToolId::from("t-1"),
            },
            &mut state,
        );
        let _ = mgr.apply(
            StreamEvent::ToolResult {
                tool_name: "search".to_string(),
                tool_id: ToolId::from("t-1"),
                is_error: false,
                duration_ms: 120,
            },
            &mut state,
        );

        // More text.
        let _ = mgr.apply(
            StreamEvent::TextDelta("Found it.\n".to_string()),
            &mut state,
        );

        // Turn completes.
        let _ = mgr.apply(
            StreamEvent::TurnComplete {
                outcome: TurnOutcome {
                    text: "Hi there! Found it.".to_string(),
                    nous_id: NousId::from("syn"),
                    session_id: "s1".into(),
                    model: "claude-opus-4-6".to_string(),
                    tool_calls: 1,
                    input_tokens: 200,
                    output_tokens: 80,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    error: None,
                },
            },
            &mut state,
        );

        // Verify final state.
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].role, MessageRole::User);
        assert_eq!(state.messages[1].role, MessageRole::Assistant);
        assert_eq!(state.messages[1].content, "Hi there!\nFound it.\n");
        assert_eq!(state.messages[1].tool_calls, 1);
        assert!(!state.streaming.is_streaming);
    }
}
