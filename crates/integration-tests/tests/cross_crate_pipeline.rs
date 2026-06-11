//! Cross-crate integration tests: full message pipeline end-to-end.
//!
//! Validates the wiring between crates: HTTP → pylon → nous actor → pipeline
//! stages → LLM mock → tool execution → session persistence → SSE response.
//!
//! Each test uses the shared `integration_tests::harness::TestHarness`,
//! extended with multi-response mock providers for tool-use round trips and
//! recall injection.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use hermeneus::provider::LlmProvider;
use hermeneus::test_utils::MockProvider;
use hermeneus::types::*;
use integration_tests::harness::{TestHarness, body_json, body_string};

/// Captures all LLM requests for inspection.
struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(text: &str, captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_capture".to_owned(),
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
            },
            captured,
        }
    }
}

impl LlmProvider for CapturingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            Ok(self.response.clone())
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-capturing"
    }
}

/// Returns different responses for successive calls (`tool_use` then `end_turn`).
struct SequentialMockProvider {
    responses: Mutex<Vec<CompletionResponse>>,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl SequentialMockProvider {
    fn new(
        responses: Vec<CompletionResponse>,
        captured: Arc<Mutex<Vec<CompletionRequest>>>,
    ) -> Self {
        Self {
            responses: Mutex::new(responses),
            captured,
        }
    }
}

impl LlmProvider for SequentialMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            #[expect(
                clippy::expect_used,
                reason = "test mock: empty responses means a test bug"
            )]
            let mut responses = self.responses.lock().expect("lock poisoned");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                // Return the last response for all subsequent calls
                Ok(responses[0].clone())
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-sequential"
    }
}

async fn build_capturing(text: &str) -> (TestHarness, Arc<Mutex<Vec<CompletionRequest>>>) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new(text, Arc::clone(&captured));
    let harness = TestHarness::build_with_provider(Box::new(provider)).await;
    (harness, captured)
}

/// Parse SSE body into (`event_type`, data) pairs.
fn parse_sse_events(body: &str) -> Vec<(String, String)> {
    let mut events = Vec::new();
    let mut current_event = String::new();
    let mut current_data = String::new();

    for line in body.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            ev.clone_into(&mut current_event);
        } else if let Some(data) = line.strip_prefix("data: ") {
            data.clone_into(&mut current_data);
        } else if line.is_empty() && !current_event.is_empty() {
            events.push((current_event.clone(), current_data.clone()));
            current_event.clear();
            current_data.clear();
        }
    }
    // Capture final event if no trailing blank line
    if !current_event.is_empty() {
        events.push((current_event, current_data));
    }
    events
}

#[path = "cross_crate_pipeline/auth_errors.rs"]
mod auth_errors;
#[path = "cross_crate_pipeline/concurrent.rs"]
mod concurrent;
#[path = "cross_crate_pipeline/lifecycle.rs"]
mod lifecycle;
#[path = "cross_crate_pipeline/wiring.rs"]
mod wiring;
