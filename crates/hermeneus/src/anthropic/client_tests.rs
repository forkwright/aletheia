#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: HashMap string-key indexing; key presence is the assertion under test"
)]
use std::borrow::Cow;
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use koina::secret::SecretString;

use super::*;
use crate::anthropic::pricing::{estimate_cost, model_family};
use crate::error::Error;
use crate::models::BACKOFF_MAX_MS;
use crate::provider::{DeploymentTarget, LlmProvider, MatchKind, ProviderConfig};
use crate::types::{CompletionRequest, Content, Message, Role};

fn test_config_with(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider_type: "anthropic".to_owned(),
        api_key: Some(SecretString::from("test-key")),
        base_url: Some(base_url.to_owned()),
        default_model: None,
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        concurrency: crate::concurrency::ConcurrencyConfig::default(),
        pricing: HashMap::new(),
        cc_mimicry: None,
        prompt_cache_mode: crate::provider::PromptCacheMode::Disabled,
        deployment_target: crate::provider::DeploymentTarget::Cloud,
        name: None,
        models: Vec::new(),
    }
}

fn test_request() -> CompletionRequest {
    CompletionRequest {
        model: "claude-opus-4-20250514".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 128,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    }
}

fn valid_wire_response_json() -> serde_json::Value {
    serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello from test"}],
        "model": "claude-opus-4-20250514",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    })
}

#[test]
fn from_config_missing_api_key() {
    let config = ProviderConfig {
        api_key: None,
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("should fail without key");
    assert!(
        matches!(err, Error::ProviderInit { .. }),
        "expected ProviderInit, got: {err:?}"
    );
}

#[test]
fn from_config_empty_api_key() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("")),
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("should fail with empty key");
    assert!(
        matches!(err, Error::ProviderInit { .. }),
        "expected ProviderInit, got: {err:?}"
    );
}

#[test]
fn from_config_rejects_spoofed_loopback_without_leaking_credentials() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("sk-secret-5055")),
        base_url: Some("http://operator:url-secret@127.0.0.1.evil.example/v1".to_owned()),
        ..ProviderConfig::default()
    };
    let err = AnthropicProvider::from_config(&config).expect_err("spoofed host must fail");
    let message = err.to_string();

    assert!(message.contains("HTTPS"), "unexpected error: {message}");
    assert!(
        message.contains("127.0.0.1.evil.example"),
        "diagnostic should keep the rejected host: {message}"
    );
    assert!(
        !message.contains("url-secret"),
        "diagnostic must redact URL userinfo: {message}"
    );
    assert!(
        !message.contains("sk-secret-5055"),
        "diagnostic must not include API keys: {message}"
    );
}

#[test]
fn with_credential_provider_rejects_spoofed_loopback_without_leaking_credentials() {
    use std::sync::Arc;

    let config = ProviderConfig {
        base_url: Some("http://operator:url-secret@localhost.evil.example/v1".to_owned()),
        ..ProviderConfig::default()
    };
    let credential_provider = Arc::new(StaticCredentialProvider {
        key: SecretString::from("dynamic-secret-5055"),
    });

    let err = AnthropicProvider::with_credential_provider(credential_provider, &config)
        .expect_err("spoofed host must fail");
    let message = err.to_string();

    assert!(message.contains("HTTPS"), "unexpected error: {message}");
    assert!(
        message.contains("localhost.evil.example"),
        "diagnostic should keep the rejected host: {message}"
    );
    assert!(
        !message.contains("url-secret"),
        "diagnostic must redact URL userinfo: {message}"
    );
    assert!(
        !message.contains("dynamic-secret-5055"),
        "diagnostic must not include credential provider secrets: {message}"
    );
}

#[test]
fn from_config_valid() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://custom.api.example.com".to_owned()),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let debug = format!("{provider:?}");
    // codequality:ignore -- debug output of provider struct contains base_url, not credential values
    assert!(
        debug.contains("custom.api.example.com"),
        "debug should show base_url: {debug}"
    );
}

#[test]
fn from_config_custom_models_claim_routing() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://compat.api.example.com".to_owned()),
        name: Some("kimi-coding".to_owned()),
        models: vec!["kimi-for-coding".to_owned()],
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.name(), "kimi-coding");
    // WHY (#5259): config-owned model IDs are no longer exposed through
    // `supported_models()` as `&[&str]` (that would require leaking). They are
    // still reachable via `supported_model_list()` and routing uses
    // `match_specificity()`.
    assert_eq!(
        provider
            .supported_model_list()
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["kimi-for-coding"]
    );
    assert!(provider.supports_model("kimi-for-coding"));
    assert_eq!(
        provider.match_specificity("kimi-for-coding"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity(koina::models::names::opus()),
        Some(MatchKind::CatchAll),
        "custom-model instance catches claude-* at lower precedence"
    );
}

#[test]
fn from_config_default_models_and_name_unchanged() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.name(), "anthropic");
    assert!(provider.supports_model("claude-opus-4-6"));
    // WHY (#4881): built-in Anthropic catalog models are exact matches,
    // not catch-all, so they win over broad claude-* providers.
    assert_eq!(
        provider.match_specificity("claude-opus-4-6"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity("claude-future-family-model"),
        Some(MatchKind::CatchAll)
    );
    assert_eq!(provider.match_specificity("kimi-for-coding"), None);
}

#[test]
fn from_config_deployment_target_propagates() {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        base_url: Some("https://compat.api.example.com".to_owned()),
        deployment_target: DeploymentTarget::LocalHosted,
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert_eq!(provider.deployment_target(), DeploymentTarget::LocalHosted);
}

#[tokio::test]
async fn complete_success() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let response = provider.complete(&test_request()).await.expect("complete");
    assert_eq!(
        response.id, "msg_test",
        "response id should match wire response"
    );
    assert_eq!(
        response.stop_reason,
        crate::types::StopReason::EndTurn,
        "stop reason should be EndTurn"
    );
    assert_eq!(
        response.usage.input_tokens, 10,
        "input tokens should match wire response"
    );
}

#[tokio::test]
#[expect(
    deprecated,
    reason = "set_linger(Duration::ZERO) sends RST immediately and does not block; test-only"
)]
async fn complete_streaming_retries_pre_content_connection_reset() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("read listener address");
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_server = Arc::clone(&call_count);

    let good_sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_sse_retry\",\"model\":\"claude-opus-4-20250514\",\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Ok\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let server = tokio::spawn(async move {
        {
            let (mut socket, _) = listener.accept().await.expect("accept attempt 1");
            call_count_server.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0_u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.expect("read request 1");
                if n == 0 || buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                .await
                .expect("write attempt 1 headers");
            socket
                .set_linger(Some(Duration::ZERO))
                .expect("set reset linger");
            drop(socket);
        }

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
                .expect("write retry response");
        }
    });

    let mut config = test_config_with(&format!("http://{addr}"));
    config.retry_policy.max_retries = 1;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let mut events = Vec::new();
    let response = provider
        .complete_streaming(&test_request(), |event| events.push(event))
        .await
        .expect("streaming should retry after pre-content reset");

    server.await.expect("server task completes");

    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "pre-content connection reset should be retried once"
    );
    assert_eq!(response.id, "msg_sse_retry");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextDelta { text } if text == "Ok")),
        "retry response text delta should be emitted: {events:?}"
    );
}

/// #3406: every outbound request must carry a training opt-out header.
/// Sovereignty default — not configurable.
#[tokio::test]
async fn request_carries_training_optout_header() {
    use wiremock::matchers::header_exists;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header_exists("anthropic-disable-training"))
        .and(header_exists("anthropic-training-opt-out"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    provider
        .complete(&test_request())
        .await
        .expect("complete ok");
}

/// #3410: when `prompt_cache_mode` = `Disabled` (default), the serialized
/// request body must contain no `cache_control` markers regardless of
/// what the caller put on the `CompletionRequest`.
#[tokio::test]
async fn disabled_prompt_cache_strips_cache_control_markers() {
    use wiremock::matchers::body_string_contains;

    let server = MockServer::start().await;

    // Any request that slips a `cache_control` marker through fails to
    // match and causes the mock's .expect(1) check to fire.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    // Negative mock: if the body ever contains cache_control we would
    // route to this 500 responder instead.
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("cache_control"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    assert_eq!(
        config.prompt_cache_mode,
        crate::provider::PromptCacheMode::Disabled,
        "test fixture default must be Disabled"
    );
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    // Caller sets every cache flag; provider must scrub them.
    let mut req = test_request();
    req.cache_system = true;
    req.cache_tools = true;
    req.cache_turns = true;

    provider.complete(&req).await.expect("complete ok");
}

/// #3410: when `prompt_cache_mode` = `Ephemeral`, caller-provided cache
/// flags are honored and `cache_control` markers appear in the wire body.
#[tokio::test]
async fn ephemeral_prompt_cache_preserves_cache_control_markers() {
    use wiremock::matchers::body_string_contains;

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_string_contains("cache_control"))
        .respond_with(ResponseTemplate::new(200).set_body_json(valid_wire_response_json()))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.prompt_cache_mode = crate::provider::PromptCacheMode::Ephemeral;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");

    let mut req = test_request();
    // system: field with cache_system = true triggers the array form with
    // cache_control on the block.
    req.system = Some("operator system prompt".to_owned());
    req.cache_system = true;

    provider.complete(&req).await.expect("complete ok");
}

#[test]
fn prepare_request_borrows_when_no_mutation_is_required() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let req = test_request();

    let prepared = provider.prepare_request(&req);
    let Cow::Borrowed(_prepared) = prepared else {
        panic!("default API-key request with clear cache flags should be borrowed");
    };
}

#[test]
fn prepare_request_clones_only_to_scrub_cache_flags() {
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let mut req = test_request();
    req.cache_system = true;
    req.cache_tools = true;
    req.cache_turns = true;

    let prepared = provider.prepare_request(&req);
    let Cow::Owned(prepared) = prepared else {
        panic!("cache flag scrub requires an owned prepared request");
    };

    assert!(!prepared.cache_system);
    assert!(!prepared.cache_tools);
    assert!(!prepared.cache_turns);
    assert!(req.cache_system);
    assert!(req.cache_tools);
    assert!(req.cache_turns);
}

#[test]
fn prepare_request_oauth_identity_and_cache_policy_share_one_owned_pass() {
    use std::sync::Arc;

    use koina::credential::{Credential, CredentialProvider, CredentialSource};

    struct OAuthProvider;

    impl CredentialProvider for OAuthProvider {
        fn get_credential(&self) -> Option<Credential> {
            Some(Credential {
                secret: SecretString::from("oauth-access-token"),
                source: CredentialSource::OAuth,
            })
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "oauth"
        }
    }

    let config = ProviderConfig {
        base_url: Some("https://api.anthropic.com".to_owned()),
        cc_mimicry: Some(true),
        prompt_cache_mode: crate::provider::PromptCacheMode::Disabled,
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::with_credential_provider(Arc::new(OAuthProvider), &config)
        .expect("valid config");
    let mut req = test_request();
    req.system = Some("operator bootstrap".to_owned());
    req.cache_system = true;
    req.cache_tools = true;
    req.cache_turns = true;

    let prepared = provider.prepare_request(&req);
    let Cow::Owned(prepared) = prepared else {
        panic!("OAuth identity rewrite should require one owned prepared request");
    };

    assert_eq!(
        prepared.system.as_deref(),
        Some("You are Claude Code, Anthropic's official CLI for Claude.")
    );
    let first = prepared
        .messages
        .first()
        .expect("system context should be inserted before user messages");
    assert_eq!(first.role, Role::User);
    assert!(first.content.text().contains("[System context]"));
    assert!(first.content.text().contains("operator bootstrap"));
    assert!(!prepared.cache_system);
    assert!(!prepared.cache_tools);
    assert!(!prepared.cache_turns);
    assert_eq!(req.system.as_deref(), Some("operator bootstrap"));
    assert!(req.cache_system);
    assert!(req.cache_tools);
    assert!(req.cache_turns);
}

#[tokio::test]
async fn complete_auth_failure_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid api key"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.retry_policy.max_retries = 2;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::AuthFailed { .. }),
        "expected AuthFailed, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_bad_request_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "bad input"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = test_config_with(&server.uri());
    config.retry_policy.max_retries = 2;
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ApiError { status: 400, .. }),
        "expected ApiError 400, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_rate_limited_no_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": "rate limited"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::RateLimited { .. }),
        "expected RateLimited, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_server_error_no_retry() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal server error"))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ApiError { status: 500, .. }),
        "expected ApiError 500, got: {err:?}"
    );
}

#[tokio::test]
async fn complete_malformed_body() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config_with(&server.uri());
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    let err = provider
        .complete(&test_request())
        .await
        .expect_err("should fail");
    assert!(
        matches!(err, Error::ParseResponse { .. }),
        "expected ParseResponse, got: {err:?}"
    );
}
