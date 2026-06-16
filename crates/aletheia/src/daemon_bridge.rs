//! Bridge from daemon tasks to nous actors.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use nous::handle::{DEFAULT_SEND_TIMEOUT, NousHandle};
use nous::manager::NousManager;
use oikonomos::bridge::DaemonBridge;
use oikonomos::runner::ExecutionResult;
use tokio_util::sync::CancellationToken;

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
    ) -> Pin<Box<dyn Future<Output = oikonomos::error::Result<ExecutionResult>> + Send + '_>> {
        self.send_prompt_with_cancel(nous_id, session_key, prompt, CancellationToken::new())
    }

    fn send_prompt_with_cancel(
        &self,
        nous_id: &str,
        session_key: &str,
        prompt: &str,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = oikonomos::error::Result<ExecutionResult>> + Send + '_>> {
        let nous_id = nous_id.to_owned();
        let session_key = session_key.to_owned();
        let prompt = prompt.to_owned();

        Box::pin(async move {
            let Some(handle) = self.nous_manager.get(&nous_id) else {
                tracing::warn!(nous_id = %nous_id, "daemon bridge: nous actor not found");
                return Ok(ExecutionResult::failed(Some(format!(
                    "nous actor {nous_id} not found"
                ))));
            };

            match send_turn_with_cancel_timeout(
                &handle,
                &session_key,
                &prompt,
                DEFAULT_SEND_TIMEOUT,
                cancel,
            )
            .await
            {
                Ok(result) => {
                    tracing::debug!(
                        nous_id = %nous_id,
                        content_len = result.content.len(),
                        "daemon bridge: prompt delivered"
                    );
                    Ok(ExecutionResult::success(Some(result.content)))
                }
                Err(e) => {
                    tracing::warn!(
                        nous_id = %nous_id,
                        error = %e,
                        "daemon bridge: turn failed"
                    );
                    Ok(ExecutionResult::failed(Some(format!("turn failed: {e}"))))
                }
            }
        })
    }
}

// WHY: `NousHandle::send_turn_with_cancel` is generic over `impl Into<String>`
// and takes `Option<String>` for the session id. Wrapping it in a private,
// non-generic async fn keeps the bridge future's type concrete and avoids
// leaking generics into the trait object return type.
async fn send_turn_with_cancel_timeout(
    handle: &NousHandle,
    session_key: &str,
    prompt: &str,
    timeout: Duration,
    cancel: CancellationToken,
) -> nous::error::Result<nous::pipeline::TurnResult> {
    handle
        .send_turn_with_cancel(
            session_key.to_owned(),
            None::<String>,
            prompt.to_owned(),
            timeout,
            cancel,
        )
        .await
}
