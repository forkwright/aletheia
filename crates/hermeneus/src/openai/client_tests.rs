#![expect(clippy::unwrap_used, reason = "test assertions")]

use std::time::Duration;

use koina::secret::SecretString;

use super::*;
use crate::models::BACKOFF_MAX_MS;

#[test]
fn rejects_plain_http_to_non_loopback() {
    let config = OpenAiProviderConfig {
        base_url: "http://evil.example.com/v1".to_owned(),
        ..Default::default()
    };
    let err = OpenAiProvider::new(config).unwrap_err();
    assert!(err.to_string().contains("HTTPS"));
}

#[test]
fn accepts_loopback_http() {
    let config = OpenAiProviderConfig {
        name: "local".to_owned(),
        base_url: "http://127.0.0.1:8088/v1".to_owned(),
        models: vec!["qwen".to_owned()],
        ..Default::default()
    };
    let provider = OpenAiProvider::new(config).unwrap();
    assert_eq!(provider.name(), "local");
    assert!(provider.supports_model("qwen"));
}

#[test]
fn accepts_https() {
    let config = OpenAiProviderConfig {
        name: "cloud".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        models: vec!["gpt-4o".to_owned()],
        ..Default::default()
    };
    let provider = OpenAiProvider::new(config).unwrap();
    assert!(provider.supports_model("gpt-4o"));
    assert!(!provider.supports_model("nonexistent"));
}

#[test]
fn deployment_target_propagates_from_config() {
    // WHY(#3736): regression — deployment_target was accepted in TOML but
    // dropped before recall filtering, treating non-cloud providers as Cloud.
    let local_hosted = OpenAiProvider::new(OpenAiProviderConfig {
        name: "local-hosted".to_owned(),
        base_url: "http://127.0.0.1:8088/v1".to_owned(),
        models: vec!["qwen".to_owned()],
        deployment_target: DeploymentTarget::LocalHosted,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        local_hosted.deployment_target(),
        DeploymentTarget::LocalHosted,
        "LocalHosted config must propagate through OpenAiProvider::deployment_target()"
    );

    let embedded = OpenAiProvider::new(OpenAiProviderConfig {
        name: "embedded".to_owned(),
        base_url: "http://127.0.0.1:8089/v1".to_owned(),
        models: vec!["logismos".to_owned()],
        deployment_target: DeploymentTarget::Embedded,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        embedded.deployment_target(),
        DeploymentTarget::Embedded,
        "Embedded config must propagate through OpenAiProvider::deployment_target()"
    );

    let default_cloud = OpenAiProvider::new(OpenAiProviderConfig {
        name: "cloud".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        models: vec!["gpt-4o".to_owned()],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        default_cloud.deployment_target(),
        DeploymentTarget::Cloud,
        "omitted deployment_target must default to Cloud (sovereignty-safe)"
    );
}

#[test]
fn deployment_target_ordering_is_sovereignty_safe() {
    // WHY(#3736): the recall rule is `sensitivity <= target`; reordering
    // the enum would let Cloud providers admit `Internal`/`Confidential` facts.
    assert!(DeploymentTarget::Cloud < DeploymentTarget::LocalHosted);
    assert!(DeploymentTarget::LocalHosted < DeploymentTarget::Embedded);
}

#[test]
fn first_party_openai_rejects_missing_api_key() {
    let config = OpenAiProviderConfig {
        name: "openai".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        api_family: OpenAiApiFamily::Responses,
        ..Default::default()
    };
    let err = OpenAiProvider::new(config).unwrap_err();
    assert!(
        err.to_string().contains("API key"),
        "error must mention missing API key: {err}"
    );
}

#[test]
fn openai_compatible_allows_missing_api_key() {
    let config = OpenAiProviderConfig {
        name: "local".to_owned(),
        base_url: "http://127.0.0.1:8088/v1".to_owned(),
        models: vec!["qwen".to_owned()],
        api_family: OpenAiApiFamily::ChatCompletions,
        ..Default::default()
    };
    let provider = OpenAiProvider::new(config).unwrap();
    assert_eq!(provider.name(), "local");
    assert!(provider.supports_model("qwen"));
}

#[tokio::test]
async fn configured_concurrency_threshold_changes_provider_limiter_behavior() {
    let provider = OpenAiProvider::new(OpenAiProviderConfig {
        name: "limited".to_owned(),
        base_url: "http://127.0.0.1:8088/v1".to_owned(),
        concurrency: crate::concurrency::ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 20,
            increase_step: 1,
            decrease_factor: 0.5,
            ewma_alpha: 0.0,
            latency_threshold_secs: 0.1,
        },
        ..Default::default()
    })
    .unwrap();

    let permit = provider.concurrency.acquire().await;
    permit.finish_with_latency(RequestOutcome::Success, Duration::from_millis(500));

    assert_eq!(
        provider.concurrency.limit(),
        5,
        "latency above configured threshold should reduce the provider limiter"
    );
}

#[test]
fn first_party_openai_accepts_api_key() {
    let config = OpenAiProviderConfig {
        name: "openai".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        api_key: Some(SecretString::from("sk-test")),
        api_family: OpenAiApiFamily::Responses,
        models: vec!["gpt-4o".to_owned()],
        ..Default::default()
    };
    let provider = OpenAiProvider::new(config).unwrap();
    assert!(provider.supports_model("gpt-4o"));
}

#[test]
fn backoff_delay_respects_retry_after() {
    let err = crate::error::RateLimitedSnafu {
        retry_after_ms: 5000_u64,
    }
    .build();
    let delay = crate::RetryPolicy::default().delay(1, Some(&err));
    assert_eq!(
        delay,
        Duration::from_secs(5),
        "should use retry-after from rate limit error"
    );
}

#[test]
fn backoff_delay_exponential_growth() {
    let policy = crate::RetryPolicy::default();
    let d1 = policy.delay(1, None);
    let d2 = policy.delay(2, None);
    let d3 = policy.delay(3, None);
    assert!(d1 < d2, "attempt 2 delay should exceed attempt 1");
    assert!(d2 < d3, "attempt 3 delay should exceed attempt 2");
    assert!(
        d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
        "delay should be capped near BACKOFF_MAX_MS"
    );
}

#[test]
fn config_models_exposed_without_leaking() {
    // WHY (#5259): dynamic OpenAI-compatible model lists are config-owned.
    // `supported_models()` must not leak them as `&[&str]`; the diagnostic
    // `supported_model_list()` returns owned `Cow` values that are freed
    // when the provider drops.
    let config = OpenAiProviderConfig {
        name: "cloud".to_owned(),
        base_url: "https://api.openai.com/v1".to_owned(),
        models: vec!["gpt-4o".to_owned()],
        ..Default::default()
    };
    let provider = OpenAiProvider::new(config).unwrap();
    assert!(provider.supported_models().is_empty());
    let list = provider.supported_model_list();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list.first().map(std::convert::AsRef::as_ref),
        Some("gpt-4o")
    );
    assert!(provider.supports_model("gpt-4o"));
    assert_eq!(provider.match_specificity("gpt-4o"), Some(MatchKind::Exact));
}

#[test]
fn repeated_construction_does_not_leak_model_storage() {
    // WHY (#5259): long-running harnesses may construct providers many
    // times (config reloads, tests, multi-instance startup). Each
    // construction must be able to free its model list.
    for i in 0..100 {
        let config = OpenAiProviderConfig {
            name: format!("cloud-{i}"),
            base_url: "https://api.openai.com/v1".to_owned(),
            models: vec![format!("gpt-model-{i}")],
            ..Default::default()
        };
        let provider = OpenAiProvider::new(config).unwrap();
        assert!(provider.supports_model(&format!("gpt-model-{i}")));
        // provider drops here, freeing the owned model list.
    }
}

mod cache_metrics_tests {
    use std::time::{Duration, Instant};

    use koina::metrics::MetricsRegistry;

    use super::{record_nonstream_success, record_stream_success};
    use crate::metrics::register;
    use crate::types::{
        CompletionRequest, CompletionResponse, Content, ContentBlock, Message, Role, StopReason,
        Usage,
    };

    fn fresh_registry() -> MetricsRegistry {
        let r = MetricsRegistry::new();
        r.with_registry(register);
        r
    }

    fn encode(r: &MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    fn request() -> CompletionRequest {
        CompletionRequest {
            model: "gpt-4o".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
                cache_breakpoint: false,
            }],
            ..Default::default()
        }
    }

    fn response_with_cache() -> CompletionResponse {
        CompletionResponse {
            id: "resp_1".to_owned(),
            model: "gpt-4o".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "hi".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 30,
                cache_write_tokens: 10,
            },
            cost_usd: None,
            duration_ms: None,
        }
    }

    #[test]
    fn nonstream_success_records_cache_tokens() {
        let r = fresh_registry();
        let start = Instant::now()
            .checked_sub(Duration::from_millis(10))
            .unwrap();
        let mut response = response_with_cache();
        record_nonstream_success(
            start,
            0,
            "openai-test",
            "responses",
            &request(),
            &mut response,
            0.001,
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_llm_cache_tokens_total{provider=\"openai-test\",direction=\"read\"} 30"
            ),
            "missing cache read metrics: {out}"
        );
        assert!(
            out.contains(
                "aletheia_llm_cache_tokens_total{provider=\"openai-test\",direction=\"write\"} 10"
            ),
            "missing cache write metrics: {out}"
        );
    }

    #[test]
    fn stream_success_records_cache_tokens() {
        let r = fresh_registry();
        let start = Instant::now()
            .checked_sub(Duration::from_millis(10))
            .unwrap();
        let mut response = response_with_cache();
        record_stream_success(
            start,
            0,
            "openai-stream-test",
            &request(),
            &mut response,
            0.001,
        );
        let out = encode(&r);
        assert!(
            out.contains(
                "aletheia_llm_cache_tokens_total{provider=\"openai-stream-test\",direction=\"read\"} 30"
            ),
            "missing cache read metrics: {out}"
        );
        assert!(
            out.contains(
                "aletheia_llm_cache_tokens_total{provider=\"openai-stream-test\",direction=\"write\"} 10"
            ),
            "missing cache write metrics: {out}"
        );
    }
}
