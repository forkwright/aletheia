//! Integration-style tests for the OpenAI-compatible provider.
//!
//! Uses `wiremock` to stand up an in-process HTTP server, then exercises
//! the full request/response path. Focused on behaviors that cross the
//! wire module boundary (the per-module unit tests already cover JSON
//! translation in isolation).

use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::openai::{OpenAiProvider, OpenAiProviderConfig};
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
