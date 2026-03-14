//! Shared types for SSE and streaming events.
//!
//! These mirror the TUI's type system but use `compact_str::CompactString`
//! newtypes consistent with workspace conventions.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Newtype for agent (nous) identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NousId(CompactString);

impl NousId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NousId {
    fn from(s: String) -> Self {
        Self(CompactString::from(s))
    }
}

impl From<&str> for NousId {
    fn from(s: &str) -> Self {
        Self(CompactString::from(s))
    }
}

impl std::ops::Deref for NousId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for NousId {
    fn eq(&self, other: &str) -> bool {
        self.0.as_str() == other
    }
}

/// Newtype for session identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(CompactString);

impl SessionId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self(CompactString::from(s))
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self(CompactString::from(s))
    }
}

impl std::ops::Deref for SessionId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// Newtype for turn identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnId(CompactString);

impl From<String> for TurnId {
    fn from(s: String) -> Self {
        Self(CompactString::from(s))
    }
}

impl From<&str> for TurnId {
    fn from(s: &str) -> Self {
        Self(CompactString::from(s))
    }
}

impl std::ops::Deref for TurnId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// Newtype for tool call identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolId(CompactString);

impl From<String> for ToolId {
    fn from(s: String) -> Self {
        Self(CompactString::from(s))
    }
}

impl From<&str> for ToolId {
    fn from(s: &str) -> Self {
        Self(CompactString::from(s))
    }
}

impl std::ops::Deref for ToolId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// Global SSE events from `GET /api/v1/events`.
///
/// These provide cross-session awareness: agent status changes, session
/// lifecycle events, and memory distillation progress.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SseEvent {
    Connected,
    Disconnected,
    Init {
        active_turns: Vec<ActiveTurn>,
    },
    TurnBefore {
        nous_id: NousId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    TurnAfter {
        nous_id: NousId,
        session_id: SessionId,
    },
    ToolCalled {
        nous_id: NousId,
        tool_name: String,
    },
    ToolFailed {
        nous_id: NousId,
        tool_name: String,
        error: String,
    },
    StatusUpdate {
        nous_id: NousId,
        status: String,
    },
    SessionCreated {
        nous_id: NousId,
        session_id: SessionId,
    },
    SessionArchived {
        nous_id: NousId,
        session_id: SessionId,
    },
    DistillBefore {
        nous_id: NousId,
    },
    DistillStage {
        nous_id: NousId,
        stage: String,
    },
    DistillAfter {
        nous_id: NousId,
    },
    Ping,
}

/// Per-session streaming events from `POST /api/v1/sessions/stream`.
///
/// These carry the LLM response for a single turn: text deltas, tool
/// invocations, plan execution, and completion/abort signals.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TurnStart {
        session_id: SessionId,
        nous_id: NousId,
        turn_id: TurnId,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        tool_name: String,
        tool_id: ToolId,
    },
    ToolResult {
        tool_name: String,
        tool_id: ToolId,
        is_error: bool,
        duration_ms: u64,
    },
    ToolApprovalRequired {
        turn_id: TurnId,
        tool_name: String,
        tool_id: ToolId,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    ToolApprovalResolved {
        tool_id: ToolId,
        decision: String,
    },
    TurnComplete {
        outcome: TurnOutcome,
    },
    TurnAbort {
        reason: String,
    },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTurn {
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    #[serde(rename = "turnId")]
    pub turn_id: TurnId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub text: String,
    #[serde(rename = "nousId")]
    pub nous_id: NousId,
    #[serde(rename = "sessionId")]
    pub session_id: SessionId,
    pub model: String,
    #[serde(rename = "toolCalls", default)]
    pub tool_calls: u32,
    #[serde(rename = "inputTokens", default)]
    pub input_tokens: u32,
    #[serde(rename = "outputTokens", default)]
    pub output_tokens: u32,
    #[serde(rename = "cacheReadTokens", default)]
    pub cache_read_tokens: u32,
    #[serde(rename = "cacheWriteTokens", default)]
    pub cache_write_tokens: u32,
    #[serde(default)]
    pub error: Option<String>,
}

/// Connection state for the global SSE stream.
/// Dioxus components read this to render connection indicators.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state, not yet attempted.
    Disconnected,
    /// Actively receiving events.
    Connected,
    /// Reconnecting after failure. `attempt` counts consecutive failures.
    Reconnecting { attempt: u32 },
}

/// State of a single streaming turn, suitable for driving a Dioxus signal.
#[derive(Debug, Clone, Default)]
pub struct StreamingState {
    /// Accumulated response text (not yet committed to history).
    pub text: String,
    /// Accumulated extended thinking output.
    pub thinking: String,
    /// Active tool calls in progress.
    pub tool_calls: Vec<ToolCallInfo>,
    /// Whether the stream is actively receiving deltas.
    pub is_streaming: bool,
    /// Turn ID if a turn is in progress.
    pub turn_id: Option<TurnId>,
    /// Error message if the stream errored.
    pub error: Option<String>,
}

/// Information about a single tool invocation during streaming.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub tool_name: String,
    pub tool_id: ToolId,
    pub is_error: bool,
    pub duration_ms: Option<u64>,
    pub completed: bool,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn nous_id_from_string() {
        let id = NousId::from("syn".to_string());
        assert_eq!(id.as_str(), "syn");
    }

    #[test]
    fn nous_id_deref_eq() {
        let id = NousId::from("syn");
        assert_eq!(&*id, "syn");
        assert!(id == *"syn");
    }

    #[test]
    fn session_id_from_str() {
        let id = SessionId::from("sess-1");
        assert_eq!(id.as_str(), "sess-1");
    }

    #[test]
    fn turn_outcome_deserialize() {
        let json = r#"{
            "text": "response",
            "nousId": "syn",
            "sessionId": "s1",
            "model": "claude-opus-4-6",
            "toolCalls": 3,
            "inputTokens": 100,
            "outputTokens": 50
        }"#;
        let outcome: TurnOutcome = serde_json::from_str(json).unwrap();
        assert_eq!(outcome.text, "response");
        assert_eq!(outcome.tool_calls, 3);
        assert_eq!(outcome.input_tokens, 100);
        assert_eq!(outcome.cache_read_tokens, 0);
    }

    #[test]
    fn active_turn_deserialize() {
        let json = r#"{"nousId":"syn","sessionId":"s1","turnId":"t1"}"#;
        let turn: ActiveTurn = serde_json::from_str(json).unwrap();
        assert_eq!(&*turn.nous_id, "syn");
        assert_eq!(&*turn.session_id, "s1");
        assert_eq!(&*turn.turn_id, "t1");
    }

    #[test]
    fn streaming_state_default() {
        let state = StreamingState::default();
        assert!(!state.is_streaming);
        assert!(state.text.is_empty());
        assert!(state.turn_id.is_none());
        assert!(state.error.is_none());
    }

    #[test]
    fn connection_state_equality() {
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Connected, ConnectionState::Disconnected);
        assert_eq!(
            ConnectionState::Reconnecting { attempt: 3 },
            ConnectionState::Reconnecting { attempt: 3 },
        );
    }

    #[test]
    fn tool_call_info_construction() {
        let info = ToolCallInfo {
            tool_name: "read_file".to_string(),
            tool_id: ToolId::from("t1"),
            is_error: false,
            duration_ms: Some(150),
            completed: true,
        };
        assert_eq!(info.tool_name, "read_file");
        assert!(!info.is_error);
        assert!(info.completed);
    }
}
