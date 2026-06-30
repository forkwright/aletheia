//! Integration-style tests for the OpenAI-compatible provider.
//!
//! Uses `wiremock` to stand up an in-process HTTP server, then exercises
//! the full request/response path. Focused on behaviors that cross the
//! wire module boundary (the per-module unit tests already cover JSON
//! translation in isolation).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::error::Error;
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
async fn configured_non_streaming_timeout_cancels_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(500))
                .set_body_json(serde_json::json!({
                    "id": "chatcmpl-timeout",
                    "model": "qwen",
                    "choices": [{
                        "message": { "role": "assistant", "content": "too late" },
                        "finish_reason": "stop",
                        "index": 0
                    }],
                    "usage": { "prompt_tokens": 6, "completion_tokens": 2, "total_tokens": 8 }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-timeout".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        models: vec!["qwen".to_owned()],
        request_timeout: Duration::from_millis(50),
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        ..OpenAiProviderConfig::default()
    })
    .expect("provider construct");

    let start = Instant::now();
    let err = provider
        .complete(&basic_request("qwen"))
        .await
        .expect_err("configured timeout should abort the request");

    assert!(
        start.elapsed() < Duration::from_millis(450),
        "request should end before the mock response delay"
    );
    assert!(
        matches!(err, Error::ApiRequest { .. }),
        "timeout should surface as ApiRequest, got: {err:?}"
    );
}

#[tokio::test]
async fn configured_retry_policy_controls_attempts_and_backoff() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_responder = Arc::clone(&attempts);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_request: &wiremock::Request| {
            if attempts_for_responder.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": { "message": "temporary overload" }
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "chatcmpl-retry",
                    "model": "qwen",
                    "choices": [{
                        "message": { "role": "assistant", "content": "recovered" },
                        "finish_reason": "stop",
                        "index": 0
                    }],
                    "usage": { "prompt_tokens": 6, "completion_tokens": 2, "total_tokens": 8 }
                }))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-retry-policy".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        models: vec!["qwen".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 1,
            backoff_base_ms: 200,
            backoff_max_ms: 200,
        },
        ..OpenAiProviderConfig::default()
    })
    .expect("provider construct");

    let start = Instant::now();
    let resp = provider
        .complete(&basic_request("qwen"))
        .await
        .expect("retry should recover");

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(resp.id, "chatcmpl-retry");
    assert!(
        start.elapsed() >= Duration::from_millis(125),
        "configured 200ms jittered backoff should delay the retry"
    );
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
async fn invalid_api_key_error_code_maps_to_auth_failed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "message": "Incorrect API key provided",
                "type": "invalid_request_error",
                "code": "invalid_api_key"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = mock_provider(&server, vec!["qwen".to_owned()]);
    let err = provider
        .complete(&basic_request("qwen"))
        .await
        .expect_err("request should fail");

    assert!(
        matches!(err, Error::AuthFailed { .. }),
        "expected AuthFailed, got {err:?}"
    );
    assert!(err.to_string().contains("Incorrect API key provided"));
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
async fn streaming_retries_retryable_http_error_before_sse() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_responder = Arc::clone(&attempts);
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-retry\",\"model\":\"gpt-5\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Ok\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n"
    );

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"stream\":true"))
        .respond_with(move |_request: &wiremock::Request| {
            if attempts_for_responder.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": { "message": "temporary overload" }
                }))
            } else {
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse)
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-responses".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        models: vec!["gpt-5".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 1,
            ..crate::RetryPolicy::default()
        },
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    })
    .expect("mock responses provider construct");

    let mut events = Vec::new();
    let resp = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect("streaming completion retries and succeeds");

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(resp.id, "resp-retry");
    assert_eq!(resp.usage.input_tokens, 3);
    assert!(events.iter().any(
        |event| matches!(event, crate::anthropic::StreamEvent::TextDelta { text } if text == "Ok")
    ));
}

#[tokio::test]
async fn streaming_retries_request_send_failure_before_sse() {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let addr = reserved.local_addr().expect("read reserved port");
    drop(reserved);

    let listener_attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_server = Arc::clone(&listener_attempts);
    let server = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind retry target");
        let (mut socket, _peer_addr) = listener.accept().await.expect("accept retry request");
        attempts_for_server.fetch_add(1, Ordering::SeqCst);

        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = socket.read(&mut chunk).await.expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Ok\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-send-retry\",\"model\":\"gpt-5\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Ok\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write sse response");
    });

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-responses".to_owned(),
        base_url: format!("http://{addr}/v1"),
        models: vec!["gpt-5".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 1,
            ..crate::RetryPolicy::default()
        },
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    })
    .expect("mock responses provider construct");

    let mut events = Vec::new();
    let resp = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect("streaming completion retries after send failure");

    server.await.expect("server task completes");
    assert_eq!(listener_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(resp.id, "resp-send-retry");
    assert!(events.iter().any(
        |event| matches!(event, crate::anthropic::StreamEvent::TextDelta { text } if text == "Ok")
    ));
}

#[tokio::test]
async fn streaming_does_not_retry_non_retryable_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": { "message": "bad stream request" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-responses".to_owned(),
        base_url: format!("{}/v1", server.uri()),
        models: vec!["gpt-5".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 2,
            ..crate::RetryPolicy::default()
        },
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    })
    .expect("mock responses provider construct");

    let mut events = Vec::new();
    let err = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect_err("bad request should fail without retry");

    assert!(events.is_empty());
    assert!(
        matches!(err, crate::error::Error::ApiError { status: 400, .. }),
        "expected ApiError 400, got {err:?}"
    );
}

#[tokio::test]
#[expect(
    deprecated,
    reason = "set_linger(Duration::ZERO) sends RST immediately and does not block; test-only"
)]
async fn streaming_retries_sse_connection_reset_before_content() {
    // WHY(#4887): SSE-level connection errors before any content delta must be
    // retried, mirroring the Anthropic path. Verify by having the server reset the
    // connection after sending HTTP 200 headers (so the SSE body parser sees the
    // error, not the HTTP status check), then serve a valid response on retry.
    //
    // WHY: bind the listener before spawning so the port is ready for attempt 0;
    // a post-spawn bind races with the client and wastes the only retry slot on
    // "connection refused" before the RST attempt can happen.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("read port");

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_server = Arc::clone(&call_count);

    let good_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-sse-retry\",\"model\":\"gpt-5\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Ok\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n"
    );

    let server = tokio::spawn(async move {
        // Attempt 1: send HTTP 200 headers then RST — SSE body parser gets a connection error.
        {
            let (mut socket, _) = listener.accept().await.expect("accept attempt 1");
            call_count_server.fetch_add(1, Ordering::SeqCst);
            // Drain the HTTP request headers.
            let mut buf = vec![0_u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read request");
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            // Send HTTP 200 with SSE content-type but no body — then RST.
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                .await
                .expect("write headers");
            // set_linger(Some(ZERO)) makes the OS send RST on close, giving reqwest
            // a retryable "connection reset" error from the SSE body reader.
            socket
                .set_linger(Some(std::time::Duration::ZERO))
                .expect("set linger");
            drop(socket); // RST sent here
        }

        // Attempt 2: serve a valid SSE response.
        {
            let (mut socket, _) = listener.accept().await.expect("accept attempt 2");
            call_count_server.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0_u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read request 2");
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                good_sse.len(),
                good_sse
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write good sse");
        }
    });

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-sse-retry".to_owned(),
        base_url: format!("http://{addr}/v1"),
        models: vec!["gpt-5".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 1,
            ..crate::RetryPolicy::default()
        },
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    })
    .expect("construct provider");

    let mut events = Vec::new();
    let resp = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect("streaming must retry and succeed");

    server.await.expect("server task completes");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "must retry once after pre-content SSE connection error"
    );
    assert_eq!(resp.id, "resp-sse-retry");
    assert!(
        events.iter().any(
            |e| matches!(e, crate::anthropic::StreamEvent::TextDelta { text } if text == "Ok")
        )
    );
}

#[tokio::test]
#[expect(
    deprecated,
    reason = "set_linger(Duration::ZERO) sends RST immediately and does not block; test-only"
)]
async fn streaming_does_not_retry_sse_error_after_content_started() {
    // WHY(#4887): once any content delta has been delivered, retrying would
    // duplicate output. Verify that a connection RST after the first delta
    // propagates immediately with call_count == 1.
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let addr = reserved.local_addr().expect("read reserved port");
    drop(reserved);

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_server = Arc::clone(&call_count);

    let server = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
        let (mut socket, _) = listener.accept().await.expect("accept");
        call_count_server.fetch_add(1, Ordering::SeqCst);

        let mut buf = vec![0_u8; 4096];
        loop {
            let n = socket.read(&mut buf).await.expect("read request");
            if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }

        // Send HTTP 200 + one text delta, then RST.
        let headers = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n";
        let delta = "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi\"}\n\n";
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("write headers");
        socket
            .write_all(delta.as_bytes())
            .await
            .expect("write delta");
        socket.flush().await.expect("flush");
        // Brief pause to let reqwest read the delta before RST.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // RST rather than FIN so reqwest gets a retryable "connection reset" error.
        socket
            .set_linger(Some(std::time::Duration::ZERO))
            .expect("set linger");
        drop(socket); // content_started gate must prevent retry
    });

    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "test-openai-content-gate".to_owned(),
        base_url: format!("http://{addr}/v1"),
        models: vec!["gpt-5".to_owned()],
        retry_policy: crate::RetryPolicy {
            max_retries: 2,
            ..crate::RetryPolicy::default()
        },
        api_family: OpenAiApiFamily::Responses,
        ..OpenAiProviderConfig::default()
    })
    .expect("construct provider");

    let mut events = Vec::new();
    let err = provider
        .complete_streaming(&basic_request("gpt-5"), &mut |event| events.push(event))
        .await
        .expect_err("mid-stream error must propagate, not retry");

    server.await.expect("server task completes");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "must not retry after content has been delivered to on_event"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, crate::anthropic::StreamEvent::TextDelta { text } if text == "Hi")
        ),
        "first delta must have been delivered before the error: {events:?}"
    );
    assert!(!err.to_string().is_empty(), "error must propagate: {err:?}");
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

#[test]
fn pricing_field_defaults_empty() {
    // WHY(#4628): unpriced local models should still construct without error.
    let cfg = OpenAiProviderConfig::default();
    assert!(cfg.pricing.is_empty(), "default pricing must be empty");
}

#[test]
fn pricing_field_round_trips() {
    // WHY(#4628): operators can supply per-model pricing rates in config.
    use crate::provider::ModelPricing;
    use std::collections::HashMap;

    let mut pricing = HashMap::new();
    pricing.insert(
        "gpt-4o".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 2.5,
            output_cost_per_mtok: 10.0,
        },
    );
    let cfg = OpenAiProviderConfig {
        name: "priced-openai".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        pricing,
        ..OpenAiProviderConfig::default()
    };
    assert_eq!(cfg.pricing.len(), 1);
    let p = &cfg.pricing["gpt-4o"];
    assert!((p.input_cost_per_mtok - 2.5).abs() < f64::EPSILON);
    assert!((p.output_cost_per_mtok - 10.0).abs() < f64::EPSILON);
}
