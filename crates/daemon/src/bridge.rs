//! Bridge trait for daemon → nous communication.

use std::future::Future;
use std::pin::Pin;

use crate::runner::ExecutionResult;

/// Allows daemon tasks to send prompts to a nous actor without the daemon
/// crate depending on nous directly.
///
/// Implemented in the binary crate where both daemon and nous are available.
/// Uses boxed futures for object safety (`Arc<dyn DaemonBridge>`).
pub trait DaemonBridge: Send + Sync {
    /// Send a prompt to a nous actor for processing within a given session.
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>>;
}

// Trait implementations are in a separate module to avoid trait-impl colocation.
mod bridge_impl;

pub use bridge_impl::NoopBridge;

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use crate::bridge::DaemonBridge;
    use crate::bridge_impl::NoopBridge;

    #[tokio::test]
    async fn noop_bridge_returns_not_success() {
        let bridge = NoopBridge;
        let result = bridge
            .send_prompt("test-nous", "test-session", "hello")
            .await
            .expect("should not error");
        assert!(!result.success, "NoopBridge should return success=false");
    }

    #[tokio::test]
    async fn noop_bridge_output_contains_no_bridge() {
        let bridge = NoopBridge;
        let result = bridge
            .send_prompt("test-nous", "test-session", "hello")
            .await
            .expect("should not error");
        let output = result.output.expect("should have output");
        assert!(
            output.contains("no bridge configured"),
            "output should mention no bridge configured, got: {output}"
        );
    }
}
