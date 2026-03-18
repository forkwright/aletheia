//! Model fallback chain for LLM requests.
//!
//! On retryable failures (429, 503, 529, timeout), tries alternative models
//! in order before giving up. Non-retryable errors (400, 401) propagate
//! immediately without attempting fallbacks.

use crate::error::Result;
use crate::provider::LlmProvider;
use crate::types::{CompletionRequest, CompletionResponse};

/// Configuration for the model fallback chain.
#[derive(Debug, Clone)]
pub struct FallbackConfig {
    /// Ordered fallback models to try after the primary fails.
    pub fallback_models: Vec<String>,
    /// How many times to call the provider for each model before moving
    /// to the next. Each call uses the provider's internal retry logic.
    pub retries_before_fallback: u32,
}

/// Execute a completion request with model fallback on retryable errors.
///
/// Tries the primary model (from `request.model`) up to `retries_before_fallback`
/// times. If all attempts fail with retryable errors, tries each fallback model
/// in order. Non-retryable errors (auth failures, invalid requests) propagate
/// immediately.
///
/// # Errors
///
/// Returns the last error if all models in the chain fail, or the first
/// non-retryable error encountered.
pub async fn complete_with_fallback(
    provider: &dyn LlmProvider,
    request: &CompletionRequest,
    config: &FallbackConfig,
) -> Result<CompletionResponse> {
    let primary = &request.model;
    let mut last_error = None;

    for attempt in 0..config.retries_before_fallback.max(1) {
        if attempt > 0 {
            tracing::warn!(
                model = %primary,
                attempt,
                "retrying primary model"
            );
        }

        match provider.complete(request).await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if !e.is_retryable() {
                    return Err(e);
                }
                tracing::warn!(
                    model = %primary,
                    attempt,
                    error = %e,
                    "primary model failed with retryable error"
                );
                last_error = Some(e);
            }
        }
    }

    for fallback_model in &config.fallback_models {
        tracing::warn!(
            primary = %primary,
            fallback = %fallback_model,
            reason = %last_error.as_ref().map_or("unknown", |_| "retryable error on previous model"),
            "falling back to alternative model"
        );

        let mut fallback_req = request.clone();
        fallback_req.model = fallback_model.clone();

        match provider.complete(&fallback_req).await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                if !e.is_retryable() {
                    return Err(e);
                }
                tracing::warn!(
                    model = %fallback_model,
                    error = %e,
                    "fallback model failed with retryable error"
                );
                last_error = Some(e);
            }
        }
    }

    // SAFETY: the loop above always executes at least once (max(1)), so
    // `last_error` is always `Some` when we reach this point.
    Err(last_error.unwrap_or_else(|| {
        crate::error::ApiRequestSnafu {
            message: "all models in fallback chain failed".to_owned(),
        }
        .build()
    }))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::error::{self, ApiErrorContext};
    use crate::types::*;

    struct MockFallbackProvider {
        responses: Mutex<Vec<Result<CompletionResponse>>>,
        call_models: Mutex<Vec<String>>,
        call_count: AtomicU32,
    }

    impl MockFallbackProvider {
        fn new(responses: Vec<Result<CompletionResponse>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                call_models: Mutex::new(Vec::new()),
                call_count: AtomicU32::new(0),
            }
        }

        fn call_count(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }

        fn called_models(&self) -> Vec<String> {
            self.call_models.lock().unwrap().clone()
        }
    }

    impl LlmProvider for MockFallbackProvider {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.call_models.lock().unwrap().push(request.model.clone());
            let result = {
                let mut responses = self.responses.lock().unwrap();
                if responses.is_empty() {
                    Err(error::ApiRequestSnafu {
                        message: "no more mock responses".to_owned(),
                    }
                    .build())
                } else {
                    responses.remove(0)
                }
            };
            Box::pin(async move { result })
        }

        fn supported_models(&self) -> &[&str] {
            &["primary-model", "fallback-1", "fallback-2"]
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "mock-fallback"
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "returns Result to match Vec<Result> in mock"
    )]
    fn ok_response(model: &str) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            id: "resp-1".to_owned(),
            model: model.to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "response".to_owned(),
                citations: None,
            }],
            usage: Usage::default(),
        })
    }

    fn retryable_error() -> Result<CompletionResponse> {
        Err(error::RateLimitedSnafu {
            retry_after_ms: 100_u64,
        }
        .build())
    }

    fn non_retryable_error() -> Result<CompletionResponse> {
        Err(error::AuthFailedSnafu {
            message: "invalid key".to_owned(),
        }
        .build())
    }

    fn server_error() -> Result<CompletionResponse> {
        Err(error::ApiSnafu {
            status: 503_u16,
            message: "service unavailable".to_owned(),
            context: ApiErrorContext::empty(),
        }
        .build())
    }

    fn test_request(model: &str) -> CompletionRequest {
        CompletionRequest {
            model: model.to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
            }],
            max_tokens: 1024,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn primary_succeeds_no_fallback() {
        let provider = MockFallbackProvider::new(vec![ok_response("primary-model")]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 2,
        };

        let resp = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap();

        assert_eq!(resp.model, "primary-model");
        assert_eq!(provider.call_count(), 1, "should not attempt fallback");
        assert_eq!(provider.called_models(), vec!["primary-model"]);
    }

    #[tokio::test]
    async fn falls_back_on_retryable_error() {
        let provider = MockFallbackProvider::new(vec![
            retryable_error(),
            retryable_error(),
            ok_response("fallback-1"),
        ]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 2,
        };

        let resp = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap();

        assert_eq!(resp.model, "fallback-1");
        assert_eq!(provider.call_count(), 3);
        assert_eq!(
            provider.called_models(),
            vec!["primary-model", "primary-model", "fallback-1"]
        );
    }

    #[tokio::test]
    async fn falls_back_on_server_error() {
        let provider = MockFallbackProvider::new(vec![server_error(), ok_response("fallback-1")]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 1,
        };

        let resp = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap();

        assert_eq!(resp.model, "fallback-1");
    }

    #[tokio::test]
    async fn all_models_fail_returns_last_error() {
        let provider =
            MockFallbackProvider::new(vec![retryable_error(), retryable_error(), server_error()]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 2,
        };

        let err = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap_err();

        assert!(
            err.is_retryable(),
            "last error should be the retryable server error"
        );
        assert_eq!(provider.call_count(), 3);
    }

    #[tokio::test]
    async fn non_retryable_error_skips_fallback() {
        let provider = MockFallbackProvider::new(vec![non_retryable_error()]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 2,
        };

        let err = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap_err();

        assert!(
            !err.is_retryable(),
            "auth error should propagate immediately"
        );
        assert_eq!(
            provider.call_count(),
            1,
            "should not retry or fallback on auth failure"
        );
    }

    #[tokio::test]
    async fn non_retryable_error_on_fallback_propagates() {
        let provider = MockFallbackProvider::new(vec![retryable_error(), non_retryable_error()]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned()],
            retries_before_fallback: 1,
        };

        let err = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap_err();

        assert!(
            !err.is_retryable(),
            "non-retryable fallback error should propagate"
        );
        assert_eq!(provider.call_count(), 2);
    }

    #[tokio::test]
    async fn empty_fallback_chain_returns_primary_error() {
        let provider = MockFallbackProvider::new(vec![retryable_error()]);
        let config = FallbackConfig {
            fallback_models: Vec::new(),
            retries_before_fallback: 1,
        };

        let err = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap_err();

        assert!(err.is_retryable());
        assert_eq!(provider.call_count(), 1);
    }

    #[tokio::test]
    async fn second_fallback_succeeds() {
        let provider = MockFallbackProvider::new(vec![
            retryable_error(),
            retryable_error(),
            ok_response("fallback-2"),
        ]);
        let config = FallbackConfig {
            fallback_models: vec!["fallback-1".to_owned(), "fallback-2".to_owned()],
            retries_before_fallback: 1,
        };

        let resp = complete_with_fallback(&provider, &test_request("primary-model"), &config)
            .await
            .unwrap();

        assert_eq!(resp.model, "fallback-2");
        assert_eq!(
            provider.called_models(),
            vec!["primary-model", "fallback-1", "fallback-2"]
        );
    }
}
