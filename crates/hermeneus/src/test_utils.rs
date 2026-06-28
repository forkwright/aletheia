//! Shared mock LLM provider for tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex; // kanon:ignore RUST/std-mutex-in-async

use crate::error::{self, Result};
use crate::provider::{LlmProvider, MatchKind, ProviderCredentialValidation};
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
        cost_usd: None,
        duration_ms: None,
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
    /// Override for `match_specificity`. `None` = use the default exact-match logic.
    match_kind_override: Option<MatchKind>,
    credential_validation: ProviderCredentialValidation,
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
            match_kind_override: None,
            credential_validation: ProviderCredentialValidation::Unknown,
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
            match_kind_override: None,
            credential_validation: ProviderCredentialValidation::Unknown,
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
            match_kind_override: None,
            credential_validation: ProviderCredentialValidation::Unknown,
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

    /// Override the `match_specificity` kind returned for any model in `models`.
    ///
    /// Used in tests to simulate catch-all or prefix providers without
    /// needing a real provider implementation.
    #[must_use]
    pub fn with_match_kind(mut self, kind: MatchKind) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.match_kind_override = Some(kind);
        self
    }

    /// Set the credential validation result returned by this mock.
    #[must_use]
    pub fn with_credential_validation(mut self, result: ProviderCredentialValidation) -> Self {
        // kanon:ignore RUST/pub-visibility
        self.credential_validation = result;
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
            // WHY `unwrap_or_else(|p| p.into_inner())`: a poisoned mutex
            // here means a prior test panicked; the mock is observational
            // so we recover the inner state and continue rather than
            // cascading panics across the test suite.
            self.requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());

            {
                let mut err_guard = self
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(err) = err_guard.take() {
                    return Err(err);
                }
            }

            let mut responses = self
                .responses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                // WHY `.first().cloned().ok_or_else(...)`: the mock
                // contract requires at least one configured response,
                // but returning a typed error lets the test harness
                // surface misconfiguration cleanly instead of panicking.
                responses.first().cloned().ok_or_else(|| {
                    error::ProviderInitSnafu {
                        message: "MockProvider has no responses configured".to_owned(),
                    }
                    .build()
                })
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        self.models
    }

    fn match_specificity(&self, model: &str) -> Option<crate::provider::MatchKind> {
        if self.models.contains(&model) {
            Some(self.match_kind_override.unwrap_or(MatchKind::Exact))
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        self.provider_name
    }

    fn validate_credential<'a>(
        &'a self,
        _credential: &'a koina::secret::SecretString,
    ) -> Pin<Box<dyn Future<Output = ProviderCredentialValidation> + Send + 'a>> {
        Box::pin(async move { self.credential_validation })
    }
}
