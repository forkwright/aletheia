use std::borrow::Cow;
use std::time::Duration;

use super::*;
use crate::anthropic::pricing::{estimate_cost, model_family};
use crate::models::BACKOFF_MAX_MS;
use crate::provider::{LlmProvider, MatchKind, ProviderConfig};
use crate::types::{CompletionRequest, Content, Message, Role};

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
        ..Default::default()
    }
}

#[test]
fn estimate_cost_no_pricing_returns_zero() {
    let pricing = HashMap::new();
    assert!(
        estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100).abs() < f64::EPSILON,
        "opus cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "claude-sonnet-4-20250514", 1000, 100).abs() < f64::EPSILON,
        "sonnet cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1000, 100).abs() < f64::EPSILON,
        "haiku cost should be zero with no pricing"
    );
    assert!(
        estimate_cost(&pricing, "some-unknown-model", 1000, 100).abs() < f64::EPSILON,
        "unknown model cost should be zero with no pricing"
    );
}

#[test]
fn estimate_cost_uses_config_pricing() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "custom-model".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 10.0,
            output_cost_per_mtok: 50.0,
        },
    );
    let cost = estimate_cost(&pricing, "custom-model", 1000, 100);
    assert!(
        (cost - 0.015).abs() < 0.0001,
        "cost should match expected value for custom pricing"
    );
}

#[test]
fn estimate_cost_config_overrides_default() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-opus-4-20250514".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 20.0,
            output_cost_per_mtok: 100.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100);
    assert!(
        (cost - 0.03).abs() < 0.0001,
        "operator pricing should override default"
    );
}

#[test]
fn estimate_cost_family_resolution_uses_alias_pricing() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-opus-4".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 15.0,
            output_cost_per_mtok: 75.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-opus-4-20250514", 1000, 100);
    assert!(
        (cost - 0.0225).abs() < 0.0001,
        "family alias pricing should resolve for opus"
    );
}

#[test]
fn estimate_cost_haiku_family_resolution() {
    let mut pricing = HashMap::new();
    pricing.insert(
        "claude-haiku-4-5".to_owned(),
        ModelPricing {
            input_cost_per_mtok: 0.8,
            output_cost_per_mtok: 4.0,
        },
    );
    let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1000, 100);
    assert!(
        (cost - 0.0012).abs() < 0.0001,
        "haiku family alias pricing should resolve"
    );
}

#[test]
fn estimate_cost_default_pricing_resolves_haiku() {
    // WHY: default pricing must include haiku-4-5; if absent, cost is always
    // zero and telemetry looks like free usage.
    let pricing = crate::anthropic::pricing::default_pricing();
    let cost = estimate_cost(&pricing, "claude-haiku-4-5-20251001", 1_000_000, 100_000);
    assert!(
        cost > 0.0,
        "default pricing must resolve haiku-4-5 to a non-zero cost, got {cost}"
    );
}

#[test]
fn model_family_strips_last_segment() {
    assert_eq!(
        model_family("claude-opus-4-20250514"),
        Some("claude-opus-4".to_owned())
    );
    assert_eq!(
        model_family("claude-haiku-4-5-20251001"),
        Some("claude-haiku-4-5".to_owned())
    );
    assert_eq!(model_family("no-dash"), None);
    assert_eq!(model_family(""), None);
}

#[test]
fn merge_pricing_fills_defaults_for_unconfigured_models() {
    let config = ProviderConfig {
        pricing: HashMap::new(),
        ..ProviderConfig::default()
    };
    let pricing = AnthropicProvider::merge_pricing(&config);
    let cost_opus = estimate_cost(&pricing, "claude-opus-4-20250514", 1_000_000, 100_000);
    assert!(cost_opus > 0.0, "opus should have non-zero default pricing");
    let cost_sonnet = estimate_cost(&pricing, "claude-sonnet-4-20250514", 1_000_000, 100_000);
    assert!(
        cost_sonnet > 0.0,
        "sonnet should have non-zero default pricing"
    );
}

#[test]
fn merge_pricing_empty_operator_uses_all_defaults() {
    let config = ProviderConfig::default();
    let pricing = AnthropicProvider::merge_pricing(&config);
    assert!(
        !pricing.is_empty(),
        "default pricing map should be non-empty"
    );
}

#[test]
fn backoff_delay_respects_retry_after() {
    let err = error::RateLimitedSnafu {
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
fn custom_models_exposed_without_leaking() {
    // WHY (#5259): config-owned model IDs must not be intentionally leaked.
    // `supported_models()` returns `&[]` for custom lists; the diagnostic
    // `supported_model_list()` returns owned `Cow` values freed on drop.
    use koina::secret::SecretString;
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        models: vec!["custom-model".to_owned()],
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert!(provider.supported_models().is_empty());
    let list = provider.supported_model_list();
    assert!(
        list.iter().any(|m| m.as_ref() == "custom-model"),
        "config model must be enumerable"
    );
}

#[test]
fn default_models_are_borrowed_cows() {
    // WHY (#5259): first-party/static model lists should be borrowed, not
    // leaked copies of static data.
    use koina::secret::SecretString;
    let config = ProviderConfig {
        api_key: Some(SecretString::from("test-key")),
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::from_config(&config).expect("valid config");
    assert!(!provider.supported_models().is_empty());
    let list = provider.supported_model_list();
    assert!(!list.is_empty());
    assert!(
        list.iter().all(|m| matches!(m, Cow::Borrowed(_))),
        "default Anthropic catalog must be borrowed, not owned/leaked"
    );
}

#[test]
fn repeated_construction_does_not_leak_model_storage() {
    // WHY (#5259): long-running harnesses may construct providers many times.
    // Each construction must be able to free its model list.
    use koina::secret::SecretString;
    for i in 0..100 {
        let config = ProviderConfig {
            api_key: Some(SecretString::from("test-key")),
            models: vec![format!("custom-model-{i}")],
            ..ProviderConfig::default()
        };
        let provider = AnthropicProvider::from_config(&config).expect("valid config");
        assert!(provider.supports_model(&format!("custom-model-{i}")));
        // provider drops here, freeing the owned model list.
    }
}

/// #5886: `compute_attribution` must return `None` for non-OAuth credentials.
///
/// Before the fix, `cc_profile` was set at construction time whenever the
/// endpoint was first-party + `cc_mimicry` enabled, and `compute_attribution`
/// gated only on `cc_profile` presence — not on the runtime credential source.
/// This caused the CC attribution block to be injected into API-key requests,
/// sending a misleading telemetry fingerprint.
#[test]
fn compute_attribution_returns_none_for_environment_credential() {
    use std::sync::Arc;

    use koina::credential::{Credential, CredentialProvider, CredentialSource};
    use koina::secret::SecretString;

    struct EnvKeyProvider;

    impl CredentialProvider for EnvKeyProvider {
        fn get_credential(&self) -> Option<Credential> {
            Some(Credential {
                secret: SecretString::from("sk-env-test-key"),
                source: CredentialSource::Environment,
            })
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "env-key"
        }
    }

    // Construct with first-party URL + cc_mimicry enabled (the pre-fix bug path).
    // with_credential_provider will set cc_profile = Some(...) because
    // is_first_party=true and cc_mimicry defaults to true.
    let config = ProviderConfig {
        base_url: Some("https://api.anthropic.com".to_owned()),
        cc_mimicry: Some(true),
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::with_credential_provider(Arc::new(EnvKeyProvider), &config)
        .expect("valid config");

    // cc_profile must be Some (construction-time logic is unchanged).
    assert!(
        provider.cc_profile.is_some(),
        "cc_profile should be set for first-party + cc_mimicry=true"
    );

    // The regression: compute_attribution must return None despite cc_profile
    // being Some, because the runtime credential source is Environment.
    let result = provider.compute_attribution(&test_request());
    assert!(
        result.is_none(),
        "compute_attribution must return None for CredentialSource::Environment, got: {result:?}"
    );
}

/// #5886 complement: `compute_attribution` returns `Some` for OAuth credentials.
///
/// Ensures the fix does not regress the happy path — OAuth requests still
/// get the CC attribution block.
#[test]
fn compute_attribution_returns_some_for_oauth_credential() {
    use std::sync::Arc;

    use koina::credential::{Credential, CredentialProvider, CredentialSource};
    use koina::secret::SecretString;

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
        retry_policy: crate::RetryPolicy {
            max_retries: 0,
            ..crate::RetryPolicy::default()
        },
        ..ProviderConfig::default()
    };
    let provider = AnthropicProvider::with_credential_provider(Arc::new(OAuthProvider), &config)
        .expect("valid config");

    assert!(
        provider.cc_profile.is_some(),
        "cc_profile should be set for first-party OAuth + cc_mimicry=true"
    );

    let result = provider.compute_attribution(&test_request());
    assert!(
        result.is_some(),
        "compute_attribution must return Some for CredentialSource::OAuth"
    );
}
