//! Mock dispatch engine for tests.
//!
//! [`MockEngine`] implements [`DispatchEngine`] with configurable outcomes,
//! enabling unit tests without real subprocess spawns or network calls.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use crate::engine::{
    AgentOptions, DispatchEngine, SessionEvent, SessionHandle, SessionResult, SessionSpec,
};
use crate::error::{self, Result};

// ---------------------------------------------------------------------------
// MockEngine
// ---------------------------------------------------------------------------

/// Test double for [`DispatchEngine`].
///
/// Returns pre-configured outcomes in FIFO order. Thread-safe for use in
/// async test contexts.
pub struct MockEngine {
    outcomes: Mutex<VecDeque<MockOutcome>>,
}

/// Pre-configured outcome for a mock session.
#[non_exhaustive]
pub enum MockOutcome {
    /// Session completes successfully with the given events and result.
    Success {
        /// Events yielded by `next_event()` before the stream ends.
        events: Vec<SessionEvent>,
        /// Final result returned by `wait()`.
        result: SessionResult,
    },
    /// Session fails to spawn with the given error message.
    SpawnFailure {
        /// Error detail message.
        detail: String,
    },
}

impl MockEngine {
    /// Create a mock engine that will return the given outcomes in order.
    #[must_use]
    pub fn new(outcomes: Vec<MockOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::from(outcomes)),
        }
    }

    /// Pop the next configured outcome.
    fn next_outcome(&self) -> Result<MockOutcome> {
        self.outcomes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .ok_or_else(|| {
                error::EngineSnafu {
                    detail: "MockEngine: no more configured outcomes",
                }
                .build()
            })
    }
}

impl DispatchEngine for MockEngine {
    fn spawn_session<'a>(
        &'a self,
        _spec: &'a SessionSpec,
        _options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        Box::pin(async move {
            let outcome = self.next_outcome()?;
            match outcome {
                MockOutcome::Success { events, result } => {
                    let handle = MockSessionHandle::new(result.session_id.clone(), events, result);
                    let boxed: Box<dyn SessionHandle> = Box::new(handle);
                    Ok(boxed)
                }
                MockOutcome::SpawnFailure { detail } => Err(error::EngineSnafu { detail }.build()),
            }
        })
    }

    fn resume_session<'a>(
        &'a self,
        _session_id: &'a str,
        _prompt: &'a str,
        _options: &'a AgentOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn SessionHandle>>> + Send + 'a>> {
        // WHY: Resume uses the same outcome queue as spawn. The mock doesn't
        // distinguish between spawn and resume for simplicity.
        Box::pin(async move {
            let outcome = self.next_outcome()?;
            match outcome {
                MockOutcome::Success { events, result } => {
                    let handle = MockSessionHandle::new(result.session_id.clone(), events, result);
                    let boxed: Box<dyn SessionHandle> = Box::new(handle);
                    Ok(boxed)
                }
                MockOutcome::SpawnFailure { detail } => Err(error::EngineSnafu { detail }.build()),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// MockSessionHandle
// ---------------------------------------------------------------------------

/// Test double for [`SessionHandle`].
///
/// Yields pre-configured events from a `VecDeque`, then returns `None`.
/// `wait()` returns the pre-configured result.
struct MockSessionHandle {
    session_id: String,
    events: VecDeque<SessionEvent>,
    result: Option<SessionResult>,
}

impl MockSessionHandle {
    fn new(session_id: String, events: Vec<SessionEvent>, result: SessionResult) -> Self {
        Self {
            session_id,
            events: VecDeque::from(events),
            result: Some(result),
        }
    }
}

impl SessionHandle for MockSessionHandle {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>> {
        Box::pin(async move { self.events.pop_front() })
    }

    fn wait(mut self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>> {
        Box::pin(async move {
            self.result.take().ok_or_else(|| {
                error::EngineSnafu {
                    detail: "MockSessionHandle: wait() called more than once",
                }
                .build()
            })
        })
    }

    fn abort<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.events.clear();
            Ok(())
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

    fn make_result(session_id: &str, success: bool) -> SessionResult {
        SessionResult {
            session_id: session_id.to_owned(),
            cost_usd: 0.05,
            num_turns: 3,
            duration_ms: 1000,
            success,
            result_text: Some("done".to_owned()),
            model: Some("claude-3-5-sonnet".to_owned()),
        }
    }

    fn make_spec() -> SessionSpec {
        SessionSpec {
            prompt: "test".to_owned(),
            system_prompt: None,
            cwd: None,
        }
    }

    #[tokio::test]
    async fn mock_engine_success_with_events() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![
                SessionEvent::TextDelta {
                    text: "hello".to_owned(),
                },
                SessionEvent::ToolUse {
                    name: "bash".to_owned(),
                    input: serde_json::json!({"cmd": "ls"}),
                },
            ],
            result: make_result("sess-mock", true),
        }]);

        let mut handle = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await
            .unwrap();

        let e1 = handle.next_event().await;
        assert!(matches!(e1, Some(SessionEvent::TextDelta { ref text }) if text == "hello"));

        let e2 = handle.next_event().await;
        assert!(matches!(e2, Some(SessionEvent::ToolUse { ref name, .. }) if name == "bash"));

        let e3 = handle.next_event().await;
        assert!(e3.is_none());

        let result = handle.wait().await.unwrap();
        assert_eq!(result.session_id, "sess-mock");
        assert!(result.success);
    }

    #[tokio::test]
    async fn mock_engine_spawn_failure() {
        let engine = MockEngine::new(vec![MockOutcome::SpawnFailure {
            detail: "auth failure".to_owned(),
        }]);

        let result = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await;

        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("auth failure"));
    }

    #[tokio::test]
    async fn mock_engine_multiple_sessions() {
        let engine = MockEngine::new(vec![
            MockOutcome::Success {
                events: vec![],
                result: make_result("sess-1", true),
            },
            MockOutcome::Success {
                events: vec![],
                result: make_result("sess-2", false),
            },
        ]);
        let opts = AgentOptions::new();

        let h1 = engine.spawn_session(&make_spec(), &opts).await.unwrap();
        let r1 = h1.wait().await.unwrap();
        assert_eq!(r1.session_id, "sess-1");

        let h2 = engine.spawn_session(&make_spec(), &opts).await.unwrap();
        let r2 = h2.wait().await.unwrap();
        assert_eq!(r2.session_id, "sess-2");
    }

    #[tokio::test]
    async fn mock_engine_exhausted_outcomes() {
        let engine = MockEngine::new(vec![]);

        let result = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await;

        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("no more configured outcomes")
        );
    }

    #[tokio::test]
    async fn mock_engine_resume() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![],
            result: make_result("sess-resumed", true),
        }]);

        let handle = engine
            .resume_session("sess-resumed", "continue", &AgentOptions::new())
            .await
            .unwrap();

        let result = handle.wait().await.unwrap();
        assert_eq!(result.session_id, "sess-resumed");
    }

    #[tokio::test]
    async fn mock_session_abort() {
        let engine = MockEngine::new(vec![MockOutcome::Success {
            events: vec![SessionEvent::TextDelta {
                text: "hello".to_owned(),
            }],
            result: make_result("sess-abort", true),
        }]);

        let mut handle = engine
            .spawn_session(&make_spec(), &AgentOptions::new())
            .await
            .unwrap();

        handle.abort().await.unwrap();

        let event = handle.next_event().await;
        assert!(event.is_none(), "events should be cleared after abort");
    }

    #[test]
    fn mock_engine_is_send_sync() {
        const _: fn() = || {
            fn assert<T: Send + Sync>() {}
            assert::<MockEngine>();
        };
    }
}
