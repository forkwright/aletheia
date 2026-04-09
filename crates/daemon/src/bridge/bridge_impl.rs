//! `DaemonBridge` trait implementations.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::bridge::DaemonBridge;
use crate::runner::ExecutionResult;

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

/// Wrap an `Arc<dyn DaemonBridge>` to forward to the inner bridge.
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
