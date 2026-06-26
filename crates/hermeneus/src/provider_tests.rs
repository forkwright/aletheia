#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: map key is asserted present by contains_key check above"
)]
use super::*;
#[cfg(feature = "cc-provider")]
use crate::anthropic::AnthropicProvider;
#[cfg(feature = "cc-provider")]
use crate::cc::{CcProvider, CcProviderConfig};
use crate::test_utils::MockProvider;
use crate::types::*;
#[cfg(feature = "cc-provider")]
use koina::secret::SecretString;

#[tokio::test]
async fn mock_provider_completes() {
    let provider = MockProvider::new("mock response").models(&["mock-model-v1", "mock-model-v2"]);
    let request = CompletionRequest {
        model: "mock-model-v1".to_owned(),
        system: None,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
            cache_breakpoint: false,
        }],
        max_tokens: 1024,
        tools: vec![],
        temperature: None,
        thinking: None,
        stop_sequences: vec![],
        ..Default::default()
    };

    let response = provider.complete(&request).await.unwrap();
    assert_eq!(response.id, "msg_mock");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
}

#[test]
fn supports_model_check() {
    let provider = MockProvider::new("mock response").models(&["mock-model-v1", "mock-model-v2"]);
    assert!(provider.supports_model("mock-model-v1"));
    assert!(provider.supports_model("mock-model-v2"));
    assert!(!provider.supports_model("nonexistent"));
}

#[test]
fn registry_find_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("mock response").models(&["mock-model-v1"]),
    ));

    assert!(registry.find_provider("mock-model-v1").is_some());
    assert!(registry.find_provider("nonexistent").is_none());
}

#[test]
fn registry_empty() {
    let registry = ProviderRegistry::new();
    assert!(registry.find_provider("any-model").is_none());
    assert!(registry.providers().is_empty());
}

#[test]
fn provider_config_deployment_target_defaults_to_cloud() {
    // WHY (#3404, #3413): the safe default — any unconfigured provider
    // is treated as a cloud target so the sovereignty filter only
    // admits `Public` facts until the operator explicitly opts in to a
    // lower-trust boundary.
    let config = ProviderConfig::default();
    assert_eq!(
        config.deployment_target,
        DeploymentTarget::Cloud,
        "default ProviderConfig must bind deployment_target = Cloud"
    );
}

#[test]
fn deployment_target_ordering() {
    assert!(DeploymentTarget::Cloud < DeploymentTarget::LocalHosted);
    assert!(DeploymentTarget::LocalHosted < DeploymentTarget::Embedded);
}

#[test]
fn llm_provider_default_deployment_target_is_cloud() {
    let provider = MockProvider::new("x");
    assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
}

#[test]
fn provider_config_defaults() {
    let config = ProviderConfig::default();
    assert_eq!(config.provider_type, "anthropic");
    assert_eq!(
        config.default_model.as_deref(),
        Some(crate::models::names::opus())
    );
    // WHY: Default pricing must cover the models used by background tasks.
    assert!(
        config.pricing.contains_key("claude-haiku-4-5-20251001"),
        "missing default pricing for claude-haiku-4-5-20251001"
    );
    assert!(
        config.pricing.contains_key("claude-sonnet-4-20250514"),
        "missing default pricing for claude-sonnet-4-20250514"
    );
    let haiku = &config.pricing["claude-haiku-4-5-20251001"];
    assert!(
        (haiku.input_cost_per_mtok - 1.0).abs() < f64::EPSILON,
        "unexpected haiku input price"
    );
    assert!(
        (haiku.output_cost_per_mtok - 5.0).abs() < f64::EPSILON,
        "unexpected haiku output price"
    );
}

#[test]
fn mock_provider_send_sync() {
    let provider = MockProvider::new("x");
    let result = std::thread::spawn(move || provider.name().to_owned())
        .join()
        .unwrap();
    assert_eq!(result, "mock");
}

#[test]
fn registry_health_starts_up() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MockProvider::new("mock response")));

    assert_eq!(registry.provider_health("mock"), Some(ProviderHealth::Up));
}

#[test]
fn registry_health_unknown_provider() {
    let registry = ProviderRegistry::new();
    assert_eq!(registry.provider_health("nonexistent"), None);
}

#[test]
fn registry_record_error_updates_health() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MockProvider::new("mock response")));

    let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
    registry.record_error("mock", &err);

    match registry.provider_health("mock") {
        Some(ProviderHealth::Degraded {
            consecutive_errors, ..
        }) => {
            assert_eq!(consecutive_errors, 1);
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

#[test]
fn registry_record_success_resets_health() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MockProvider::new("mock response")));

    let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
    registry.record_error("mock", &err);
    registry.record_success("mock");

    assert_eq!(registry.provider_health("mock"), Some(ProviderHealth::Up));
}

#[test]
fn find_streaming_provider_returns_none_for_mock() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MockProvider::new("mock response")));
    assert!(registry.find_streaming_provider("mock-model-v1").is_none());
}

#[test]
fn registry_record_unknown_provider_does_not_mutate_known_or_insert_unknown() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(MockProvider::new("mock response")));
    let known_health_before = registry.provider_health("mock");
    let known_provider_count_before = registry
        .providers
        .iter()
        .filter(|entry| entry.provider.name() == "mock")
        .count();
    let total_provider_count_before = registry.providers.len();

    registry.record_success("nonexistent");
    let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
    registry.record_error("nonexistent", &err);

    assert_eq!(
        registry.provider_health("mock"),
        known_health_before,
        "unknown-provider records must not affect known-provider health"
    );
    assert_eq!(
        registry
            .providers
            .iter()
            .filter(|entry| entry.provider.name() == "mock")
            .count(),
        known_provider_count_before,
        "unknown-provider records must not duplicate the known provider"
    );
    assert_eq!(
        registry.providers.len(),
        total_provider_count_before,
        "unknown-provider records must not create provider entries"
    );
    assert_eq!(
        registry.provider_health("nonexistent"),
        None,
        "unknown provider must remain absent from health lookup"
    );
}

// --- Specificity-based routing tests ---

#[test]
fn match_kind_ordering() {
    assert!(MatchKind::CatchAll < MatchKind::Prefix);
    assert!(MatchKind::Prefix < MatchKind::Exact);
    assert!(MatchKind::CatchAll < MatchKind::Exact);
}

#[test]
fn single_provider_routes_normally() {
    // (a) When only one provider is registered, the normal match still works.
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("r")
            .named("cc-mock")
            .models(&["claude-sonnet-4-20250514"])
            .with_match_kind(MatchKind::CatchAll),
    ));

    let found = registry.find_provider("claude-sonnet-4-20250514");
    assert!(found.is_some(), "single catch-all provider should match");
    assert_eq!(found.unwrap().name(), "cc-mock");
    assert!(
        registry.find_provider("claude-opus-99-unknown").is_none(),
        "model not in the mock's list should not match"
    );
}

#[test]
fn explicit_exact_wins_over_catch_all() {
    // (b) When an explicit exact-model provider AND a catch-all provider both
    // match the same model ID, the exact-model provider wins regardless of
    // registration order.

    // Register catch-all first (the order that would silently win under
    // the old first-match scheme).
    let mut registry_catch_first = ProviderRegistry::new();
    registry_catch_first.register(Box::new(
        MockProvider::new("r")
            .named("cc-catch-all")
            .models(&["claude-sonnet-4-20250514"])
            .with_match_kind(MatchKind::CatchAll),
    ));
    registry_catch_first.register(Box::new(
        MockProvider::new("r")
            .named("anthropic-exact")
            .models(&["claude-sonnet-4-20250514"])
            .with_match_kind(MatchKind::Exact),
    ));

    let found = registry_catch_first
        .find_provider("claude-sonnet-4-20250514")
        .unwrap();
    assert_eq!(
        found.name(),
        "anthropic-exact",
        "exact-model provider must win over catch-all even when registered second"
    );

    // Register exact first — same result expected.
    let mut registry_exact_first = ProviderRegistry::new();
    registry_exact_first.register(Box::new(
        MockProvider::new("r")
            .named("anthropic-exact")
            .models(&["claude-sonnet-4-20250514"])
            .with_match_kind(MatchKind::Exact),
    ));
    registry_exact_first.register(Box::new(
        MockProvider::new("r")
            .named("cc-catch-all")
            .models(&["claude-sonnet-4-20250514"])
            .with_match_kind(MatchKind::CatchAll),
    ));

    let found2 = registry_exact_first
        .find_provider("claude-sonnet-4-20250514")
        .unwrap();
    assert_eq!(
        found2.name(),
        "anthropic-exact",
        "exact-model provider must win over catch-all when registered first too"
    );
}

#[test]
fn find_provider_is_deterministic_regardless_of_registration_order() {
    // (c) Same inputs → same provider, regardless of which was registered first.
    // We run both orderings and assert the winner is always the exact-match provider.
    let models: &'static [&'static str] = &["claude-haiku-4-5-20251001"];

    for (first, second) in [
        ("exact-provider", "catch-all-provider"),
        ("catch-all-provider", "exact-provider"),
    ] {
        let mut registry = ProviderRegistry::new();
        for name in [first, second] {
            let kind = if name == "exact-provider" {
                MatchKind::Exact
            } else {
                MatchKind::CatchAll
            };
            registry.register(Box::new(
                MockProvider::new("r")
                    .named(name)
                    .models(models)
                    .with_match_kind(kind),
            ));
        }

        let Some(winner) = registry.find_provider("claude-haiku-4-5-20251001") else {
            panic!("should find a provider for claude-haiku-4-5-20251001");
        };
        assert_eq!(
            winner.name(),
            "exact-provider",
            "registration order ({first} before {second}) must not change the winner"
        );
    }
}

// WHY (#4881): real-provider fixtures below exercise the actual routing code
// paths in `AnthropicProvider::match_specificity` and `ProviderRegistry`.

/// Build an [`AnthropicProvider`] using the built-in first-party catalog.
#[cfg(feature = "cc-provider")]
fn anthropic_provider_with_builtin_catalog() -> AnthropicProvider {
    let config = ProviderConfig {
        // NOTE: test-only fixture value, not a real credential
        api_key: Some(SecretString::from("sk-test-123")),
        ..ProviderConfig::default()
    };
    AnthropicProvider::from_config(&config).unwrap()
}

/// Build a [`CcProvider`] pointing at a temporary dummy binary.
#[cfg(feature = "cc-provider")]
fn cc_provider_with_dummy_binary() -> CcProvider {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "hermeneus-cc-dummy-{}-{}.sh",
        std::process::id(),
        counter
    ));
    {
        use std::io::Write as _;
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"#!/bin/sh\n").unwrap();
    }

    let config = CcProviderConfig {
        cc_binary: Some(path.clone()),
        default_model: crate::models::names::opus().to_owned(),
        timeout: std::time::Duration::from_secs(1),
    };
    let provider = CcProvider::new(&config).unwrap();

    // The provider only needs the path to exist at construction time.
    let _ = std::fs::remove_file(&path);
    provider
}

#[cfg(feature = "cc-provider")]
#[test]
fn known_anthropic_catalog_model_routes_to_anthropic_when_cc_registered_first() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(cc_provider_with_dummy_binary()));
    registry.register(Box::new(anthropic_provider_with_builtin_catalog()));

    let found = registry
        .find_provider(koina::models::names::sonnet())
        .unwrap();
    assert_eq!(
        found.name(),
        "anthropic",
        "first-party catalog model must route to Anthropic even when CC was registered first"
    );
}

#[cfg(feature = "cc-provider")]
#[test]
fn known_anthropic_catalog_model_routes_to_anthropic_when_anthropic_registered_first() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(anthropic_provider_with_builtin_catalog()));
    registry.register(Box::new(cc_provider_with_dummy_binary()));

    let found = registry
        .find_provider(koina::models::names::haiku())
        .unwrap();
    assert_eq!(
        found.name(),
        "anthropic",
        "first-party catalog model must route to Anthropic when Anthropic was registered first"
    );
}

#[cfg(feature = "cc-provider")]
#[test]
fn unknown_claude_model_routes_to_first_catch_all_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(cc_provider_with_dummy_binary()));
    registry.register(Box::new(anthropic_provider_with_builtin_catalog()));

    let found = registry
        .find_provider("claude-future-unknown-model")
        .unwrap();
    assert_eq!(
        found.name(),
        "cc",
        "unknown claude-* IDs must fall through to the first-registered catch-all provider"
    );
}
