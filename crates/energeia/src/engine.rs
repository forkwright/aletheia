//! Dispatch engine trait: abstraction over session execution backends.
//!
//! The [`DispatchEngine`] trait targets the Anthropic Agent SDK HTTP/SSE API.
//! Implementations: `HttpEngine` (production), `MockEngine` (tests).

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::error::Result;

// ---------------------------------------------------------------------------
// DispatchEngine trait
// ---------------------------------------------------------------------------

/// Core abstraction over session execution backends.
///
/// Targets the Anthropic Agent SDK HTTP/SSE API. Production implementations
/// use HTTP+SSE streaming; test implementations return canned responses.
#[expect(
    clippy::type_complexity,
    reason = "async trait methods returning boxed trait objects require nested generics"
)]
pub trait DispatchEngine: Send + Sync {
    /// Spawn a new agent session for the given spec and options.
    fn spawn_session<'a>(
        &'a self,
        spec: &'a SessionSpec,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>;

    /// Resume an existing session with a new prompt.
    fn resume_session<'a>(
        &'a self,
        session_id: &'a str,
        prompt: &'a str,
        options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// SessionHandle trait
// ---------------------------------------------------------------------------

/// Handle to a running or completed agent session.
///
/// Provides an event stream for observing session progress, plus control
/// methods for waiting on completion or aborting.
pub trait SessionHandle: Send {
    /// The Agent SDK session identifier.
    fn session_id(&self) -> &str;

    /// Receive the next event from the session's SSE stream.
    ///
    /// Returns `None` when the stream is exhausted (session complete or errored).
    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>>;

    /// Wait for the session to complete and return the final result.
    fn wait(self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>>;

    /// Request the session to abort.
    fn abort<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// Session types
// ---------------------------------------------------------------------------

/// Specification for spawning a new agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionSpec {
    /// The prompt or task description to send to the agent.
    pub prompt: String,
    /// System prompt to prepend to the session.
    pub system_prompt: Option<String>,
    /// Working directory for the agent session.
    pub cwd: Option<String>,
}

/// Configuration options for an agent session.
///
/// Built incrementally via the builder methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AgentOptions {
    /// LLM model identifier (e.g., "claude-sonnet-4-20250514").
    pub model: Option<String>,
    /// System prompt override.
    pub system_prompt: Option<String>,
    /// Working directory for the agent.
    pub cwd: Option<String>,
    /// Maximum LLM turns before the session is stopped.
    pub max_turns: Option<u32>,
    /// Permission mode for tool execution (e.g., "plan", "auto").
    pub permission_mode: Option<String>,
}

impl AgentOptions {
    /// Create empty options with all fields unset.
    #[must_use]
    pub fn new() -> Self {
        Self {
            model: None,
            system_prompt: None,
            cwd: None,
            max_turns: None,
            permission_mode: None,
        }
    }

    /// Set the model identifier.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the system prompt.
    #[must_use]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set the maximum turn count.
    #[must_use]
    pub fn max_turns(mut self, turns: u32) -> Self {
        self.max_turns = Some(turns);
        self
    }

    /// Set the permission mode.
    #[must_use]
    pub fn permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.permission_mode = Some(mode.into());
        self
    }
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// An event from a running agent session's SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionEvent {
    /// Agent produced a text output chunk.
    TextDelta {
        /// The text content.
        text: String,
    },
    /// Agent invoked a tool.
    ToolUse {
        /// Tool name.
        name: String,
        /// Tool input as JSON.
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolResult {
        /// Tool name.
        name: String,
        /// Whether the tool succeeded.
        success: bool,
    },
    /// Session turn completed.
    TurnComplete {
        /// Turn number within the session.
        turn: u32,
    },
    /// Session encountered an error.
    Error {
        /// Error description.
        message: String,
    },
}

/// Final result of a completed session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionResult {
    /// The Agent SDK session identifier.
    pub session_id: String,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Total turns consumed.
    pub num_turns: u32,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the session completed successfully.
    pub success: bool,
    /// Final text output from the agent, if any.
    pub result_text: Option<String>,
}

impl SessionResult {
    /// Create a session result.
    ///
    /// Intended for test harnesses and mock engines that need to produce
    /// results without running a real agent session.
    #[must_use]
    pub fn new(
        session_id: String,
        cost_usd: f64,
        num_turns: u32,
        duration_ms: u64,
        success: bool,
        result_text: Option<String>,
    ) -> Self {
        Self {
            session_id,
            cost_usd,
            num_turns,
            duration_ms,
            success,
            result_text,
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn agent_options_builder() {
        let opts = AgentOptions::new()
            .model("claude-sonnet-4-20250514")
            .cwd("/tmp/work")
            .max_turns(50)
            .permission_mode("plan");

        assert_eq!(opts.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(opts.cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(opts.max_turns, Some(50));
        assert_eq!(opts.permission_mode.as_deref(), Some("plan"));
    }

    #[test]
    fn agent_options_default() {
        let opts = AgentOptions::default();
        assert!(opts.model.is_none());
        assert!(opts.max_turns.is_none());
    }

    #[test]
    fn session_spec_roundtrip() {
        let spec = SessionSpec {
            prompt: "implement feature X".to_owned(),
            system_prompt: Some("you are a coding agent".to_owned()),
            cwd: Some("/home/user/project".to_owned()),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: SessionSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt, "implement feature X");
    }

    #[test]
    fn session_event_roundtrip() {
        let event = SessionEvent::ToolUse {
            name: "read_file".to_owned(),
            input: serde_json::json!({"path": "/tmp/test.rs"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: SessionEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, SessionEvent::ToolUse { name, .. } if name == "read_file"));
    }

    #[test]
    fn session_result_roundtrip() {
        let result = SessionResult {
            session_id: "sess-abc".to_owned(),
            cost_usd: 0.42,
            num_turns: 15,
            duration_ms: 30_000,
            success: true,
            result_text: Some("done".to_owned()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SessionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "sess-abc");
        assert!(deserialized.success);
    }
}
