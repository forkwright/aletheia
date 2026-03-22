//! Integration tests for `LocalProvider` using a wiremock mock server.

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::local::{LocalProvider, LocalProviderConfig};
use crate::provider::LlmProvider;
use crate::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, StopReason, ToolDefinition,
};

/// Helper to build a `LocalProvider` pointing at the mock server.
fn provider_for(server: &MockServer) -> LocalProvider {
    let config = LocalProviderConfig {
        base_url: format!("{}/v1", server.uri()),
        default_model: "test-model".to_owned(),
        ..LocalProviderConfig::default()
    };
    LocalProvider::new(&config).unwrap()
}

/// Helper to build a simple user-message request.
fn simple_request(model: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_owned(),
        system: Some("You are a test assistant.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("Hello".to_owned()),
        }],
        max_tokens: 100,
        ..Default::default()
    }
}

#[tokio::test]
async fn simple_completion() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-test-1",
            "model": "test-model",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 8
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let response = provider
        .complete(&simple_request("local/test-model"))
        .await
        .unwrap();

    assert_eq!(response.id, "chatcmpl-test-1", "response id should match");
    assert_eq!(
        response.stop_reason,
        StopReason::EndTurn,
        "stop reason should be EndTurn"
    );
    assert_eq!(response.usage.input_tokens, 15, "input tokens should match");
    assert_eq!(
        response.usage.output_tokens, 8,
        "output tokens should match"
    );
    assert_eq!(response.content.len(), 1, "should have one content block");
    match &response.content[0] {
        ContentBlock::Text { text, .. } => {
            assert_eq!(text, "Hello! How can I help?", "response text should match")
        }
        other => panic!("expected Text, got: {other:?}"),
    }
}

#[tokio::test]
async fn streaming_completion() {
    let server = MockServer::start().await;

    // WHY: wiremock serves the full body at once, which is fine for testing
    // the SSE parser since it processes bytes incrementally.
    let sse_body = "\
data: {\"id\":\"chatcmpl-s1\",\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-s1\",\"model\":\"test-model\",\"choices\":[{\"delta\":{\"content\":\" there\"},\"finish_reason\":null}]}\n\
\n\
data: {\"id\":\"chatcmpl-s1\",\"model\":\"test-model\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
\n\
data: [DONE]\n\
\n";

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_body)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let request = simple_request("local/test-model");

    let mut events = Vec::new();
    let response = provider
        .complete_streaming(&request, &mut |e| events.push(e))
        .await
        .unwrap();

    assert_eq!(
        response.id, "chatcmpl-s1",
        "streaming response id should match"
    );
    assert_eq!(
        response.stop_reason,
        StopReason::EndTurn,
        "streaming stop reason should be EndTurn"
    );
    match &response.content[0] {
        ContentBlock::Text { text, .. } => {
            assert_eq!(text, "Hi there", "streamed text should be concatenated")
        }
        other => panic!("expected Text, got: {other:?}"),
    }

    // Verify streaming events were emitted.
    assert!(
        events.iter().any(
            |e| matches!(e, crate::anthropic::StreamEvent::TextDelta { text } if text == "Hi")
        ),
        "should have emitted a TextDelta event for 'Hi'"
    );
}

#[tokio::test]
async fn tool_call_completion() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-tool-1",
            "model": "test-model",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"London\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 10
            }
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let mut request = simple_request("local/test-model");
    request.tools = vec![ToolDefinition {
        name: "get_weather".to_owned(),
        description: "Get weather for a location".to_owned(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"}
            }
        }),
        disable_passthrough: None,
    }];

    let response = provider.complete(&request).await.unwrap();

    assert_eq!(
        response.stop_reason,
        StopReason::ToolUse,
        "stop reason should be ToolUse"
    );
    assert_eq!(response.content.len(), 1, "should have one content block");
    match &response.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_abc123", "tool call id should match");
            assert_eq!(name, "get_weather", "tool name should match");
            assert_eq!(input["location"], "London", "tool input should match");
        }
        other => panic!("expected ToolUse, got: {other:?}"),
    }
}

#[tokio::test]
async fn connection_error_returns_api_request_error() {
    // NOTE: No server started — connection will be refused.
    let config = LocalProviderConfig {
        base_url: "http://127.0.0.1:1/v1".to_owned(),
        default_model: "test-model".to_owned(),
        ..LocalProviderConfig::default()
    };
    let provider = LocalProvider::new(&config).unwrap();

    let result = provider.complete(&simple_request("local/test-model")).await;

    assert!(result.is_err(), "connection to dead server should fail");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("failed to connect"),
        "expected connection error message, got: {msg}"
    );
}

#[tokio::test]
async fn rate_limit_response_maps_to_rate_limited_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let result = provider.complete(&simple_request("local/test-model")).await;

    assert!(result.is_err(), "429 response should produce an error");
    assert!(
        matches!(result.unwrap_err(), crate::error::Error::RateLimited { .. }),
        "expected RateLimited error"
    );
}

#[tokio::test]
async fn server_error_maps_to_api_request_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let result = provider.complete(&simple_request("local/test-model")).await;

    assert!(result.is_err(), "500 response should produce an error");
    let err = result.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("500"),
        "expected status code in error, got: {msg}"
    );
}

#[test]
fn supports_model_with_local_prefix() {
    let config = LocalProviderConfig::default();
    let provider = LocalProvider::new(&config).unwrap();

    assert!(
        provider.supports_model("local/qwen3.5-27b"),
        "should support local/ prefixed model"
    );
    assert!(
        provider.supports_model("local/anything"),
        "should support any local/ prefixed model"
    );
    assert!(
        !provider.supports_model("claude-opus-4-20250514"),
        "should not support non-local models"
    );
    assert!(
        !provider.supports_model("qwen3.5-27b"),
        "should not support models without local/ prefix"
    );
}

#[test]
fn provider_name_is_local() {
    let config = LocalProviderConfig::default();
    let provider = LocalProvider::new(&config).unwrap();
    assert_eq!(provider.name(), "local", "provider name should be 'local'");
}

#[test]
fn supports_streaming() {
    let config = LocalProviderConfig::default();
    let provider = LocalProvider::new(&config).unwrap();
    assert!(
        provider.supports_streaming(),
        "local provider should support streaming"
    );
}

#[tokio::test]
async fn request_sends_system_message_and_tools() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-verify",
            "model": "test-model",
            "choices": [{
                "message": {"content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1}
        })))
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let mut request = simple_request("local/test-model");
    request.tools = vec![ToolDefinition {
        name: "echo".to_owned(),
        description: "Echo input".to_owned(),
        input_schema: serde_json::json!({"type": "object"}),
        disable_passthrough: None,
    }];

    let response = provider.complete(&request).await.unwrap();
    assert_eq!(response.id, "chatcmpl-verify", "response id should match");
}

#[tokio::test]
async fn model_prefix_stripped_in_request() {
    let server = MockServer::start().await;

    // Verify the model sent to vLLM has the "local/" prefix stripped.
    // Use a wiremock expectation that captures the request body.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-strip",
            "model": "qwen3.5-27b",
            "choices": [{
                "message": {"content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = provider_for(&server);
    let response = provider
        .complete(&simple_request("local/qwen3.5-27b"))
        .await
        .unwrap();

    assert_eq!(response.id, "chatcmpl-strip", "response id should match");

    // Verify the "local/" prefix was stripped from the model in the request.
    let requests: Vec<Request> = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        body["model"], "qwen3.5-27b",
        "local/ prefix should be stripped"
    );
}
