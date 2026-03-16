//! Bridge from daemon tasks to nous actors.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use aletheia_nous::manager::NousManager;
use aletheia_oikonomos::bridge::DaemonBridge;
use aletheia_oikonomos::runner::ExecutionResult;

pub(crate) struct NousDaemonBridge {
    nous_manager: Arc<NousManager>,
}

impl NousDaemonBridge {
    pub(crate) fn new(nous_manager: Arc<NousManager>) -> Self {
        Self { nous_manager }
    }
}

impl DaemonBridge for NousDaemonBridge {
    fn send_prompt(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
        model_override: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = aletheia_oikonomos::error::Result<ExecutionResult>> + Send + '_>>
    {
        let nous_id = nous_id.to_owned();
        let session_key = session_key.to_owned();
        let prompt = prompt.to_owned();
        let model_override = model_override.map(ToOwned::to_owned);

        Box::pin(async move {
            let Some(handle) = self.nous_manager.get(&nous_id).cloned() else {
                tracing::warn!(nous_id = %nous_id, "daemon bridge: nous actor not found");
                return Ok(ExecutionResult {
                    success: false,
                    output: Some(format!("nous actor {nous_id} not found")),
                });
            };

            match handle
                .send_turn_with_model(&session_key, &prompt, model_override.as_deref())
                .await
            {
                Ok(result) => {
                    tracing::debug!(
                        nous_id = %nous_id,
                        content_len = result.content.len(),
                        "daemon bridge: prompt delivered"
                    );
                    Ok(ExecutionResult {
                        success: true,
                        output: Some(result.content),
                    })
                }
                Err(e) => {
                    tracing::warn!(
                        nous_id = %nous_id,
                        error = %e,
                        "daemon bridge: turn failed"
                    );
                    Ok(ExecutionResult {
                        success: false,
                        output: Some(format!("turn failed: {e}")),
                    })
                }
            }
        })
    }
}
