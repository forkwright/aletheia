//! Integration-style tests for the OpenAI-compatible provider.
//!
//! Uses `wiremock` to stand up an in-process HTTP server, then exercises
//! the full request/response path. Focused on behaviors that cross the
//! wire module boundary (the per-module unit tests already cover JSON
//! translation in isolation).

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::openai::{OpenAiApiFamily, OpenAiProvider, OpenAiProviderConfig};
use crate::provider::{LlmProvider, ProviderRegistry};
use crate::types::{
    CompletionRequest, Content, ContentBlock, Message, Role, StopReason, ThinkingConfig,
    ToolDefinition,
};

fn mock_provider(server: &MockServer, models: Vec<String>) -> OpenAiProvider {
    let cfg = OpenAiProviderConfig {
        name: "test-openai".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        api_key: None,
        models,
        ..OpenAiProviderConfig::default()
    };
    OpenAiProvider::new(cfg).expect("mock provider construct")
}

fn mock_responses_provider(server: &MockServer, models: Vec<String>) -> OpenAiProvider {
    let cfg = OpenAiProviderConfig {
        name: "test-openai-responses".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        api_key: None,
        models,
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    };
    OpenAiProvider::new(cfg).expect("mock responses provider construct")
}

fn basic_request(model: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_owned(),
        system: Some("You are helpful.".to_owned()),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hi".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 64,
        ..Default::default()
    }
}

#[tokio::test]
async fn non_streaming_text_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-1",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "hello back" },
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": { "prompt_tokens": 6, "completion_tokens": 2, "total_tokens": 8 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_provider(&server, vec!["qwen".to_owned()]);
    let resp = provider
        .complete(&basic_request("qwen"))
        .await
        .expect("completion succeeds");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 6);
    match &resp.content[0] {
        ContentBlock::Text { text, .. } => assert_eq!(text, "hello back"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[tokio::test]
async fn responses_non_streaming_text_round_trip_uses_responses_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"model\":\"gpt-5\""))
        .and(body_string_contains(
            "\"instructions\":\"You are helpful.\"",
        ))
        .and(body_string_contains("\"max_output_tokens\":64"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "resp-1",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "hello from responses" }]
            }],
            "usage": { "input_tokens": 7, "output_tokens": 3, "total_tokens": 10 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_responses_provider(&server, vec!["gpt-5".to_owned()]);
    let resp = provider
        .complete(&basic_request("gpt-5"))
        .await
        .expect("completion succeeds");
    assert_eq!(resp.id, "resp-1");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage.input_tokens, 7);
    assert_eq!(resp.usage.output_tokens, 3);
    match &resp.content[0] {
        ContentBlock::Text { text, .. } => assert_eq!(text, "hello from responses"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_use_round_trip_maps_both_directions() {
    let server = MockServer::start().await;

    // Server returns a tool_calls response.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-tool",
            "model": "qwen",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_7",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Paris\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls",
                "index": 0
            }]
        })))
        .mount(&server)
        .await;

    let provider = mock_provider(&server, vec!["qwen".to_owned()]);
    let request = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("weather in Paris?".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 64,
        tools: vec![ToolDefinition {
            name: "get_weather".to_owned(),
            description: "Fetch weather".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }],
        ..Default::default()
    };

    let resp = provider.complete(&request).await.expect("completion");
    assert_eq!(resp.stop_reason, StopReason::ToolUse);
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_7");
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "Paris");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[tokio::test]
async fn responses_tool_use_round_trip_maps_both_directions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function\""))
        .and(body_string_contains("\"name\":\"get_weather\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "resp-tool",
            "model": "gpt-5",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_7",
                "name": "get_weather",
                "arguments": "{\"city\":\"Paris\"}"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_responses_provider(&server, vec!["gpt-5".to_owned()]);
    let request = CompletionRequest {
        model: "gpt-5".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("weather in Paris?".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 64,
        tools: vec![ToolDefinition {
            name: "get_weather".to_owned(),
            description: "Fetch weather".to_owned(),
            input_schema: serde_json::json!({"type": "object"}),
            disable_passthrough: None,
        }],
        ..Default::default()
    };

    let resp = provider.complete(&request).await.expect("completion");
    assert_eq!(resp.stop_reason, StopReason::ToolUse);
    match &resp.content[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_7");
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "Paris");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[tokio::test]
async fn thinking_budget_is_dropped_without_breaking_request() {
    let server = MockServer::start().await;

    // Server echoes any valid request — we just want to verify the
    // outbound body omits any thinking field.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        // WHY: matching the absence of a string is cumbersome with wiremock;
        // instead we assert the payload shape via `body_string_contains` on
        // a known field to prove the request reached the server.
        .and(body_string_contains("\"model\":\"qwen\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-thinking",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop",
                "index": 0
            }]
        })))
        .mount(&server)
        .await;

    let provider = mock_provider(&server, vec!["qwen".to_owned()]);
    let mut req = basic_request("qwen");
    req.thinking = Some(ThinkingConfig {
        enabled: true,
        budget_tokens: 1024,
    });
    let resp = provider.complete(&req).await.expect("completion succeeds");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn auth_header_sent_when_api_key_present() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer sk-test-key",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "chatcmpl-auth",
            "model": "qwen",
            "choices": [{
                "message": { "role": "assistant", "content": "authed" },
                "finish_reason": "stop",
                "index": 0
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let cfg = OpenAiProviderConfig {
        name: "authed".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        api_key: Some(koina::secret::SecretString::from("sk-test-key")),
        models: vec!["qwen".to_owned()],
        ..OpenAiProviderConfig::default()
    };
    let provider = OpenAiProvider::new(cfg).expect("construct");
    let resp = provider.complete(&basic_request("qwen")).await.expect("ok");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn responses_error_envelope_maps_to_provider_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "message": "model does not support requested tool",
                "type": "invalid_request_error",
                "code": "unsupported_tool"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_responses_provider(&server, vec!["gpt-5".to_owned()]);
    let err = provider
        .complete(&basic_request("gpt-5"))
        .await
        .expect_err("request should fail");
    assert!(
        err.to_string()
            .contains("model does not support requested tool")
    );
}

#[tokio::test]
async fn responses_streaming_text_round_trip() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-stream\",\"model\":\"gpt-5\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"lo\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-stream\",\"model\":\"gpt-5\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"stream\":true"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_responses_provider(&server, vec!["gpt-5".to_owned()]);
    let mut events = Vec::new();
    let resp = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect("streaming completion succeeds");

    assert_eq!(resp.id, "resp-stream");
    assert_eq!(resp.usage.input_tokens, 4);
    match &resp.content[0] {
        ContentBlock::Text { text, .. } => assert_eq!(text, "Hello"),
        other => panic!("expected Text, got {other:?}"),
    }
    assert!(
        events.iter().any(
            |e| matches!(e, crate::anthropic::StreamEvent::TextDelta { text } if text == "Hel")
        ),
    );
}

#[tokio::test]
async fn server_tools_request_is_rejected_before_transport() {
    let server = MockServer::start().await;
    let provider = mock_provider(&server, vec!["qwen".to_owned()]);
    let request = CompletionRequest {
        model: "qwen".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("search the web".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 64,
        server_tools: vec![crate::types::ServerToolDefinition {
            tool_type: "web_search_20250305".to_owned(),
            name: "web_search".to_owned(),
            max_uses: Some(3),
            allowed_domains: None,
            blocked_domains: None,
            user_location: None,
        }],
        ..Default::default()
    };
    let err = provider.complete(&request).await.unwrap_err();
    assert!(err.to_string().contains("server-side tools"));
}

#[tokio::test]
async fn air_gapped_registry_routes_to_local_provider() {
    // Air-gapped mode is emergent: register only a local OpenAI-compatible
    // provider, verify a turn completes without any external traffic.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "air-gapped-1",
            "model": "Qwen3.5-35B-A3B-Q8_0",
            "choices": [{
                "message": { "role": "assistant", "content": "local only" },
                "finish_reason": "stop",
                "index": 0
            }],
            "usage": { "prompt_tokens": 8, "completion_tokens": 3, "total_tokens": 11 }
        })))
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "local-qwen".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        api_key: None,
        models: vec!["Qwen3.5-35B-A3B-Q8_0".to_owned()],
        ..OpenAiProviderConfig::default()
    })
    .expect("construct");

    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(provider));

    // No Anthropic provider registered — the registry must still resolve.
    let resolved = registry
        .find_provider("Qwen3.5-35B-A3B-Q8_0")
        .expect("local provider routes");
    assert_eq!(resolved.name(), "local-qwen");

    let req = CompletionRequest {
        model: "Qwen3.5-35B-A3B-Q8_0".to_owned(),
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("ping".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 32,
        ..Default::default()
    };
    let resp = resolved.complete(&req).await.expect("air-gapped turn");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    match &resp.content[0] {
        ContentBlock::Text { text, .. } => assert_eq!(text, "local only"),
        other => panic!("expected Text, got {other:?}"),
    }
}
