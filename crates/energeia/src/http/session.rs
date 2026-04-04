//! Session handle for subprocess-based engine.
//!
//! [`ProcessSessionHandle`] implements [`SessionHandle`] by wrapping a Claude
//! CLI subprocess. It owns the child process and ensures kill-on-drop safety:
//! if the handle is dropped without calling [`SessionHandle::wait`], the
//! subprocess is killed via `start_kill()`.

use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use tokio::process::Child;

use crate::engine::{SessionEvent, SessionHandle, SessionResult};
use crate::error::{self, Result};
use crate::http::stream::EventStream;

/// Handle to a running Claude CLI subprocess session.
///
/// INVARIANT: The child process is killed on drop if `wait()` was not called.
/// This prevents zombie processes when sessions are abandoned.
pub struct ProcessSessionHandle {
    session_id: String,
    pub(crate) stream: EventStream,
    child: Option<Child>,
    start_time: Instant,
}

impl ProcessSessionHandle {
    /// Create a new handle from a spawned subprocess.
    pub(crate) fn new(child: Child, stream: EventStream, session_id: String) -> Self {
        Self {
            session_id,
            stream,
            child: Some(child),
            start_time: Instant::now(),
        }
    }

    /// Update the session ID from the stream's parsed system message.
    pub(crate) fn sync_session_id(&mut self) {
        if let Some(ref id) = self.stream.session_id {
            self.session_id.clone_from(id);
        }
    }
}

impl SessionHandle for ProcessSessionHandle {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Option<SessionEvent>> + Send + 'a>> {
        Box::pin(async move {
            let event = self.stream.next_event().await;
            // Keep session_id in sync with what the stream discovers.
            self.sync_session_id();
            event
        })
    }

    fn wait(mut self: Box<Self>) -> Pin<Box<dyn Future<Output = Result<SessionResult>> + Send>> {
        Box::pin(async move {
            // Drain remaining events to capture the result message.
            while self.stream.next_event().await.is_some() {}
            self.sync_session_id();

            // Wait for the child process to avoid zombies.
            if let Some(ref mut child) = self.child {
                let _ = child.wait().await;
            }
            // Take child so Drop doesn't kill it again.
            self.child.take();

            if self.stream.rate_limit_exceeded {
                return Err(error::BudgetExceededSnafu {
                    reason: "rate limit utilization exceeded 98%",
                }
                .build());
            }

            if let Some(result) = self.stream.wire_result.take() {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    reason = "elapsed ms fits u64 for any realistic session duration"
                )]
                let duration_ms = if result.duration_ms > 0 {
                    result.duration_ms
                } else {
                    self.start_time.elapsed().as_millis() as u64
                };

                Ok(SessionResult {
                    session_id: self.session_id.clone(),
                    cost_usd: result.total_cost_usd.unwrap_or(0.0),
                    num_turns: result.num_turns,
                    duration_ms,
                    success: !result.is_error,
                    result_text: result.result,
                })
            } else {
                Err(error::EngineSnafu {
                    detail: "subprocess exited without emitting a result message",
                }
                .build())
            }
        })
    }

    fn abort<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(ref mut child) = self.child {
                tracing::info!(session_id = %self.session_id, "aborting session subprocess");
                child.kill().await.map_err(|e| {
                    error::EngineSnafu {
                        detail: format!("failed to kill subprocess: {e}"),
                    }
                    .build()
                })?;
            }
            Ok(())
        })
    }
}

impl Drop for ProcessSessionHandle {
    fn drop(&mut self) {
        // SAFETY: start_kill() is non-async and sends SIGKILL to the child.
        // This is the kill-on-drop safety net for sessions not explicitly waited.
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_session_handle_is_send() {
        static_assertions::assert_impl_all!(ProcessSessionHandle: Send);
    }
}
