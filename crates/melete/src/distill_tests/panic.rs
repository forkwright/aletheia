//! Tests for provider panic propagation during distillation.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hermeneus::provider::LlmProvider;
use hermeneus::types::{CompletionRequest, CompletionResponse};

use super::{default_engine, sample_conversation};

/// Provider that panics on every `complete` call.
#[derive(Debug)]
struct PanicProvider;

impl LlmProvider for PanicProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = hermeneus::error::Result<CompletionResponse>> + Send + 'a>>
    {
        Box::pin(async move { panic!("provider explosion") })
    }

    fn supported_models(&self) -> &[&str] {
        &["panic-model"]
    }

    fn name(&self) -> &'static str {
        "panic-provider"
    }
}

#[tokio::test]
async fn provider_panic_propagates_and_records_backoff() {
    let engine = Arc::new(default_engine());
    let provider = PanicProvider;
    let messages = sample_conversation();

    let engine_for_task = Arc::clone(&engine);
    let handle = tokio::spawn(async move {
        engine_for_task
            .distill(&messages, "alice", &provider, 1)
            .await
    });

    let join_result = handle.await;

    assert!(
        join_result.is_err(),
        "provider panic must terminate the spawned task"
    );
    let err = join_result.expect_err("join result must be an error");
    assert!(err.is_panic(), "task must fail with a panic");
    assert!(
        engine.in_backoff(),
        "distill must record failure before re-raising the panic"
    );
}
