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
    let delay = crate::retry::backoff_delay(1, Some(&err));
    assert_eq!(
        delay,
        Duration::from_secs(5),
        "should use retry-after from rate limit error"
    );
}

#[test]
fn backoff_delay_exponential_growth() {
    let d1 = crate::retry::backoff_delay(1, None);
    let d2 = crate::retry::backoff_delay(2, None);
    let d3 = crate::retry::backoff_delay(3, None);
    assert!(d1 < d2, "attempt 2 delay should exceed attempt 1");
    assert!(d2 < d3, "attempt 3 delay should exceed attempt 2");
    assert!(
        d3 <= Duration::from_millis(BACKOFF_MAX_MS + BACKOFF_MAX_MS / 4),
        "delay should be capped near BACKOFF_MAX_MS"
    );
}
