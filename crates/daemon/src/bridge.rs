//! Bridge trait for daemon → nous communication.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::runner::ExecutionResult;

/// Allows daemon tasks to send prompts to a nous actor without the daemon
/// crate depending on nous directly.
///
/// Implemented in the binary crate where both daemon and nous are available.
/// Uses boxed futures for object safety (`Arc<dyn DaemonBridge>`).
pub trait DaemonBridge: Send + Sync {
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>>;
}

/// No-op bridge for tests and configurations without nous integration.
pub struct NoopBridge;

impl DaemonBridge for NoopBridge {
    fn send_prompt(
        &self,
        _nous_id: &str,
        _session_key: &str,
        _prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>> {
        Box::pin(async {
            tracing::warn!("no daemon bridge configured — prompt not sent");
            Ok(ExecutionResult {
                success: false,
                output: Some("no bridge configured".to_owned()),
            })
        })
    }
}

/// Wrap an `Arc<dyn DaemonBridge>` to forward to the inner trait.
impl DaemonBridge for Arc<dyn DaemonBridge> {
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<ExecutionResult>> + Send + '_>> {
        (**self).send_prompt(nous_id, session_key, prompt)
    }
}
