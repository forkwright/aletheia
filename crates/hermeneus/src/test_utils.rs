//! Shared mock LLM provider for tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async

use crate::error::{self, Result};
use crate::provider::LlmProvider;
use crate::types::{CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage};

/// Build a [`CompletionResponse`] with a single text block.
#[must_use]
pub fn make_response(text: &str) -> CompletionResponse {
    // kanon:ignore RUST/pub-visibility
    CompletionResponse {
        id: "msg_mock".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: text.to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        },
    }
}

/// Configurable mock LLM provider for tests.
///
/// Supports fixed responses, response queues, request capturing,
/// and error injection. Thread-safe via `std::sync::Mutex` (lock never
/// held across `.await`).
///
/// # Examples
///
/// Fixed text response:
/// ```ignore
/// let provider = MockProvider::new("Hello!");
/// ```
///
/// Response sequence (pops from front, repeats last):
/// ```ignore
/// let provider = MockProvider::with_responses(vec![r1, r2]);
/// ```
///
/// Error injection:
/// ```ignore
/// let provider = MockProvider::error("network timeout");
/// ```
pub struct MockProvider {
    // kanon:ignore RUST/pub-visibility
    // WHY: std::sync::Mutex is intentional: lock never held across .await
    responses: Mutex<Vec<CompletionResponse>>,
    error: Mutex<Option<error::Error>>,
    models: &'static [&'static str],
    provider_name: &'static str,
    requests: Mutex<Vec<CompletionRequest>>,
}

impl MockProvider {
    /// Create a mock that always returns the given text.
    #[must_use]
    pub fn new(text: &str) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            responses: Mutex::new(vec![make_response(text)]),
            error: Mutex::new(None),
            models: &["mock-model"],
            provider_name: "mock",
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Create a mock that returns responses in sequence.
    ///
    /// When only one response remains, it is cloned indefinitely.
    #[must_use]
    pub fn with_responses(responses: Vec<CompletionResponse>) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            responses: Mutex::new(responses),
            error: Mutex::new(None),
            models: &["mock-model"],
            provider_name: "mock",
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Create a mock that returns an [`ApiRequest`](error::ApiRequestSnafu) error.
    #[must_use]
    pub fn error(message: &str) -> Self {
        // kanon:ignore RUST/pub-visibility
        Self {
            responses: Mutex::new(Vec::new()),
            error: Mutex::new(Some(
                error::ApiRequestSnafu {
                    message: message.to_owned(),
                }
                .build(),
            )),
            models: &["mock-model"],
            provider_name: "mock",
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Set the model list returned by `supported_models`.
    #[must_use]
    pub fn models(mut self, models: &'static [&'static str]) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.models = models;
        self
    }

    /// Set the provider name returned by `name`.
    #[must_use]
    pub fn named(mut self, name: &'static str) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.provider_name = name;
        self
    }

    /// Return all captured requests (most recent last).
    #[must_use]
    pub fn captured_requests(&self) -> Vec<CompletionRequest> {
        // kanon:ignore RUST/pub-visibility
        self.requests
            .lock()
            .expect("mock request lock poisoned")
            .clone()
    }
}

impl LlmProvider for MockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(async {
            self.requests
                .lock()
                .expect("mock request lock poisoned")
                .push(request.clone());

            {
                let mut err_guard = self.error.lock().expect("mock error lock poisoned");
                if let Some(err) = err_guard.take() {
                    return Err(err);
                }
            }

            let mut responses = self.responses.lock().expect("mock response lock poisoned");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                Ok(responses
                    .first()
                    .expect("MockProvider has no responses configured")
                    .clone())
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        self.models
    }

    fn name(&self) -> &str {
        self.provider_name
    }
}
