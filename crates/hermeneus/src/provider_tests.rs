#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
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

// --- Health-aware provider selection tests ---

fn down_after_one_error_config() -> HealthConfig {
    HealthConfig {
        consecutive_failure_threshold: 1,
        down_cooldown_ms: 60_000,
    }
}

fn api_request_error() -> crate::error::Error {
    crate::error::ApiRequestSnafu {
        message: "simulated timeout".to_owned(),
    }
    .build()
}

#[test]
fn health_aware_selection_prefers_healthy_equivalent_provider() {
    // WHY: the first registered exact-match provider is down, so an equivalent
    // (same specificity) healthy provider must be selected instead.
    let mut registry = ProviderRegistry::new();
    registry.register_with_config(
        Box::new(
            MockProvider::new("first response")
                .named("first")
                .models(&["shared-model"]),
        ),
        down_after_one_error_config(),
    );
    registry.register(Box::new(
        MockProvider::new("second response")
            .named("second")
            .models(&["shared-model"]),
    ));

    registry.record_error("first", &api_request_error());

    let selected = registry
        .find_provider("shared-model")
        .expect("a healthy equivalent provider must exist");
    assert_eq!(selected.name(), "second");
}

#[test]
fn health_aware_selection_returns_none_when_all_unavailable() {
    // WHY: when every provider for a model is down, model-only routing must
    // fail rather than hand a request to an unavailable provider.
    let mut registry = ProviderRegistry::new();
    registry.register_with_config(
        Box::new(
            MockProvider::new("a")
                .named("alpha")
                .models(&["shared-model"]),
        ),
        down_after_one_error_config(),
    );
    registry.register_with_config(
        Box::new(
            MockProvider::new("b")
                .named("beta")
                .models(&["shared-model"]),
        ),
        down_after_one_error_config(),
    );

    let err = api_request_error();
    registry.record_error("alpha", &err);
    registry.record_error("beta", &err);

    assert!(
        registry.find_provider("shared-model").is_none(),
        "no healthy provider should be returned when all are down"
    );

    let err = match registry.resolve_provider("shared-model", ProviderRoute::ModelOnly) {
        Ok(provider) => panic!(
            "model-only resolution should report provider unavailability, got {}",
            provider.name()
        ),
        Err(err) => err,
    };
    match err {
        ProviderResolutionError::ProviderUnavailable { name, health } => {
            assert_eq!(name, "alpha");
            assert!(matches!(health, ProviderHealth::Down { .. }));
        }
        other @ (ProviderResolutionError::NoProvider { .. }
        | ProviderResolutionError::ProviderNotFound { .. }
        | ProviderResolutionError::ProviderDoesNotSupportModel { .. }
        | ProviderResolutionError::CapabilityMismatch { .. }) => {
            panic!("expected ProviderUnavailable, got {other}")
        }
    }
}

#[test]
fn explicit_provider_route_selects_named_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("named")
            .named("named-provider")
            .models(&["some-model"]),
    ));

    let selected = registry
        .resolve_provider("some-model", ProviderRoute::Explicit("named-provider"))
        .expect("explicit route to a healthy provider must succeed");
    assert_eq!(selected.name(), "named-provider");
}

/// Mock provider that exposes its own health tracker, for exercising
/// [`LlmProvider::health_tracker`] sharing between a provider and the
/// registry that holds it.
struct HealthSharingProvider {
    tracker: Arc<ProviderHealthTracker>,
}

impl LlmProvider for HealthSharingProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        Box::pin(async { Ok(crate::test_utils::make_response("ok")) })
    }

    fn supported_models(&self) -> &[&str] {
        &["health-sharing-model"]
    }

    fn name(&self) -> &'static str {
        "health-sharing"
    }

    fn health_tracker(&self) -> Option<Arc<ProviderHealthTracker>> {
        Some(Arc::clone(&self.tracker))
    }
}

#[test]
fn register_shares_a_providers_own_health_tracker() {
    // WHY(#5255): a provider that exposes its own tracker via
    // `health_tracker` must have the registry SHARE that exact instance
    // rather than build an independent one — otherwise routing decisions
    // and the provider's own internal circuit breaker can disagree about
    // whether the provider is healthy.
    let tracker = Arc::new(ProviderHealthTracker::new(
        "health-sharing",
        HealthConfig::default(),
    ));
    let provider = HealthSharingProvider {
        tracker: Arc::clone(&tracker),
    };

    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(provider));

    // Mutate the tracker directly, exactly as a provider's own internal
    // circuit breaker would from inside its `execute()` — NOT through the
    // registry's `record_error`.
    tracker.record_error(
        &crate::error::AuthFailedSnafu {
            message: "invalid key".to_owned(),
        }
        .build(),
    );

    // The registry must observe the SAME state through its own query path,
    // proving the two are one tracker, not two independently-updated ones.
    match registry.provider_health("health-sharing") {
        Some(ProviderHealth::Down { .. }) => {}
        other => {
            panic!("expected registry to observe the shared tracker's Down state, got {other:?}")
        }
    }
}

#[test]
fn explicit_provider_route_requires_named_provider_to_claim_model() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("named")
            .named("named-provider")
            .models(&["other-model"]),
    ));
    registry.register(Box::new(
        MockProvider::new("fallback")
            .named("fallback-provider")
            .models(&["some-model"]),
    ));

    let err = registry
        .resolve_provider("some-model", ProviderRoute::Explicit("named-provider"))
        .err()
        .expect("explicit route must not use provider that does not claim model");

    match err {
        ProviderResolutionError::ProviderDoesNotSupportModel { name, model } => {
            assert_eq!(name, "named-provider");
            assert_eq!(model, "some-model");
        }
        other => panic!("expected ProviderDoesNotSupportModel, got {other}"),
    }
}

#[test]
fn explicit_provider_route_reports_health_failure_directly() {
    // WHY: when the operator names a provider explicitly, health failures must
    // surface for that provider rather than silently falling back.
    let mut registry = ProviderRegistry::new();
    registry.register_with_config(
        Box::new(
            MockProvider::new("named")
                .named("named-provider")
                .models(&["some-model"]),
        ),
        down_after_one_error_config(),
    );
    registry.register(Box::new(
        MockProvider::new("fallback")
            .named("fallback-provider")
            .models(&["some-model"]),
    ));

    registry.record_error("named-provider", &api_request_error());

    let err = registry
        .resolve_provider("some-model", ProviderRoute::Explicit("named-provider"))
        .err()
        .expect("explicit route to a down provider must fail");

    match err {
        ProviderResolutionError::ProviderUnavailable { name, health } => {
            assert_eq!(name, "named-provider");
            assert!(matches!(health, ProviderHealth::Down { .. }));
        }
        other @ (ProviderResolutionError::NoProvider { .. }
        | ProviderResolutionError::ProviderNotFound { .. }
        | ProviderResolutionError::ProviderDoesNotSupportModel { .. }
        | ProviderResolutionError::CapabilityMismatch { .. }) => {
            panic!("expected ProviderUnavailable, got {other}")
        }
    }
}

// WHY(#5253): capability-aware resolution tests. `resolve_provider`/
// `find_provider` stay capability-blind (unchanged above); these exercise
// `resolve_provider_for_request`, the negotiation surface routing/fallback
// consult so a tool-bearing turn is never selected for an incapable provider.

#[test]
fn capability_aware_selection_prefers_capable_equivalent_provider() {
    // WHY: mirrors health_aware_selection_prefers_healthy_equivalent_provider
    // — an equivalent (same specificity) capable provider must be selected
    // over an incapable one registered first, exactly as an unhealthy one
    // is skipped in favor of a healthy equivalent.
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("incapable response")
            .named("incapable")
            .models(&["shared-model"])
            .without_tool_loop(),
    ));
    registry.register(Box::new(
        MockProvider::new("capable response")
            .named("capable")
            .models(&["shared-model"]),
    ));

    let selected = registry
        .resolve_provider_for_request(
            "shared-model",
            ProviderRoute::ModelOnly,
            ProviderCapabilities::with_tool_loop(true),
        )
        .expect("a capable equivalent provider must exist");
    assert_eq!(selected.name(), "capable");

    // WHY: capability-blind resolution is unaffected — it still returns the
    // first-registered provider regardless of capability.
    let blind = registry.find_provider("shared-model").unwrap();
    assert_eq!(blind.name(), "incapable");
}

#[test]
fn capability_aware_selection_reports_mismatch_when_every_provider_incapable() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("a")
            .named("alpha")
            .models(&["shared-model"])
            .without_tool_loop(),
    ));
    registry.register(Box::new(
        MockProvider::new("b")
            .named("beta")
            .models(&["shared-model"])
            .without_tool_loop(),
    ));

    let err = registry
        .resolve_provider_for_request(
            "shared-model",
            ProviderRoute::ModelOnly,
            ProviderCapabilities::with_tool_loop(true),
        )
        .err()
        .expect("no provider can satisfy the required capability");

    match err {
        ProviderResolutionError::CapabilityMismatch {
            name, capability, ..
        } => {
            assert_eq!(
                name, "alpha",
                "reports the first capability-incapable provider"
            );
            assert_eq!(capability, TOOL_LOOP_CAPABILITY);
        }
        other => panic!("expected CapabilityMismatch, got {other}"),
    }
}

#[test]
fn capability_aware_selection_ignores_incapable_provider_for_non_tool_request() {
    // WHY: a non-tool-bearing request has no capability requirement, so an
    // incapable provider (e.g. a seat-bridged CLI) is still eligible and
    // routes exactly as it did before this mechanism existed.
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("incapable response")
            .named("incapable")
            .models(&["shared-model"])
            .without_tool_loop(),
    ));

    let selected = registry
        .resolve_provider_for_request(
            "shared-model",
            ProviderRoute::ModelOnly,
            ProviderCapabilities::with_tool_loop(false),
        )
        .expect("an incapable provider must still serve a request that needs no capability");
    assert_eq!(selected.name(), "incapable");
}

#[test]
fn capability_aware_explicit_route_rejects_incapable_provider() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("incapable response")
            .named("named-provider")
            .models(&["some-model"])
            .without_tool_loop(),
    ));

    let err = registry
        .resolve_provider_for_request(
            "some-model",
            ProviderRoute::Explicit("named-provider"),
            ProviderCapabilities::with_tool_loop(true),
        )
        .err()
        .expect("explicit route to an incapable provider must fail");

    match err {
        ProviderResolutionError::CapabilityMismatch { name, model, .. } => {
            assert_eq!(name, "named-provider");
            assert_eq!(model, "some-model");
        }
        other => panic!("expected CapabilityMismatch, got {other}"),
    }
}

#[test]
fn capability_aware_explicit_route_allows_incapable_provider_for_non_tool_request() {
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("incapable response")
            .named("named-provider")
            .models(&["some-model"])
            .without_tool_loop(),
    ));

    let selected = registry
        .resolve_provider_for_request(
            "some-model",
            ProviderRoute::Explicit("named-provider"),
            ProviderCapabilities::with_tool_loop(false),
        )
        .expect("a non-tool-bearing explicit route must still succeed");
    assert_eq!(selected.name(), "named-provider");
}

#[test]
fn capability_mismatch_reports_after_health_when_both_present_in_tier() {
    // WHY(#5254): validates the priority `ProviderRegistry::resolve_model_only`
    // documents — an unhealthy-but-capable candidate is reported over a
    // healthy-but-incapable one, because health may recover on its own and a
    // capability gap never will. `crates/nous/src/execute/model_fallback.rs`
    // depends on this: it is what lets a fallback route correctly wait out a
    // transient health blip instead of the registry silently reporting (and a
    // capability-blind dispatcher then selecting) the incapable alternative.
    let mut registry = ProviderRegistry::new();
    registry.register_with_config(
        Box::new(
            MockProvider::new("down-but-capable")
                .named("capable-down")
                .models(&["shared-model"]),
        ),
        down_after_one_error_config(),
    );
    registry.register(Box::new(
        MockProvider::new("up-but-incapable")
            .named("incapable-up")
            .models(&["shared-model"])
            .without_tool_loop(),
    ));

    registry.record_error("capable-down", &api_request_error());

    let err = registry
        .resolve_provider_for_request(
            "shared-model",
            ProviderRoute::ModelOnly,
            ProviderCapabilities::with_tool_loop(true),
        )
        .err()
        .expect("no eligible provider: one is down, the other is incapable");

    match err {
        // WHY: proves the incapable-but-healthy provider was NOT selected as
        // a consolation prize just because it happens to be Up — the only
        // other possible outcome here would have been `Ok("incapable-up")`.
        ProviderResolutionError::ProviderUnavailable { name, .. } => {
            assert_eq!(name, "capable-down");
        }
        other => {
            panic!("expected ProviderUnavailable (health reported before capability), got {other}")
        }
    }
}

#[test]
fn provider_capabilities_required_by_reflects_request_tools() {
    let mut request = CompletionRequest {
        model: "m".to_owned(),
        ..Default::default()
    };
    assert!(!ProviderCapabilities::required_by(&request).tool_loop);

    request.tools = vec![ToolDefinition {
        name: "t".to_owned(),
        description: "d".to_owned(),
        input_schema: serde_json::json!({}),
        disable_passthrough: None,
    }];
    assert!(ProviderCapabilities::required_by(&request).tool_loop);
}

#[test]
fn provider_capabilities_satisfies_and_missing_for_agree() {
    let capable = ProviderCapabilities::with_tool_loop(true);
    let incapable = ProviderCapabilities::with_tool_loop(false);
    let required = ProviderCapabilities::with_tool_loop(true);
    let not_required = ProviderCapabilities::with_tool_loop(false);

    assert!(capable.satisfies(&required));
    assert!(capable.missing_for(&required).is_none());

    assert!(!incapable.satisfies(&required));
    assert_eq!(incapable.missing_for(&required), Some(TOOL_LOOP_CAPABILITY));

    // WHY: an incapable provider still satisfies a request that never needed
    // the capability in the first place.
    assert!(incapable.satisfies(&not_required));
    assert!(incapable.missing_for(&not_required).is_none());
}

#[test]
fn health_aware_mixed_specificity_prefers_highest_healthy() {
    // WHY: specificity ordering must still hold when all providers are healthy.
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(
        MockProvider::new("catch")
            .named("catch-provider")
            .models(&["mixed-model"])
            .with_match_kind(MatchKind::CatchAll),
    ));
    registry.register(Box::new(
        MockProvider::new("prefix")
            .named("prefix-provider")
            .models(&["mixed-model"])
            .with_match_kind(MatchKind::Prefix),
    ));
    registry.register(Box::new(
        MockProvider::new("exact")
            .named("exact-provider")
            .models(&["mixed-model"])
            .with_match_kind(MatchKind::Exact),
    ));

    let selected = registry
        .find_provider("mixed-model")
        .expect("a healthy provider must be selected");
    assert_eq!(selected.name(), "exact-provider");
}

#[test]
fn health_aware_mixed_specificity_does_not_fall_back_to_lower_tier() {
    // WHY: health is a tie-breaker within the same specificity level, not an
    // override. When all exact-match providers are down, model-only routing
    // must fail rather than silently fall back to a prefix or catch-all match.
    let mut registry = ProviderRegistry::new();
    registry.register_with_config(
        Box::new(
            MockProvider::new("catch")
                .named("catch-provider")
                .models(&["mixed-model"])
                .with_match_kind(MatchKind::CatchAll),
        ),
        down_after_one_error_config(),
    );
    registry.register_with_config(
        Box::new(
            MockProvider::new("prefix")
                .named("prefix-provider")
                .models(&["mixed-model"])
                .with_match_kind(MatchKind::Prefix),
        ),
        down_after_one_error_config(),
    );
    registry.register_with_config(
        Box::new(
            MockProvider::new("exact")
                .named("exact-provider")
                .models(&["mixed-model"])
                .with_match_kind(MatchKind::Exact),
        ),
        down_after_one_error_config(),
    );

    let err = api_request_error();
    registry.record_error("exact-provider", &err);

    assert!(
        registry.find_provider("mixed-model").is_none(),
        "must not fall back to a lower-specificity provider when the exact tier is unavailable"
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
        ..CcProviderConfig::default()
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

#[test]
fn streaming_capability_any_is_true_only_for_realtime_or_buffered() {
    assert!(!StreamingCapability::NONE.any());
    assert!(StreamingCapability::REALTIME_LIFECYCLE.any());
    assert!(
        StreamingCapability {
            buffered_single_delta: true,
            ..StreamingCapability::NONE
        }
        .any()
    );
}

#[test]
fn supports_streaming_default_derives_from_streaming_capability() {
    // WHY(#5264): `supports_streaming` must not be an independent knob —
    // a provider declaring only `streaming_capability` gets the correct
    // boolean for free, and the two cannot drift apart.
    struct RealtimeOnly;
    impl LlmProvider for RealtimeOnly {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<CompletionResponse>> + Send + 'a>,
        > {
            Box::pin(async { unimplemented!() })
        }
        fn supported_models(&self) -> &[&str] {
            &[]
        }
        fn name(&self) -> &str {
            "realtime-only"
        }
        fn streaming_capability(&self) -> StreamingCapability {
            StreamingCapability {
                realtime_deltas: true,
                ..StreamingCapability::NONE
            }
        }
    }

    assert!(RealtimeOnly.supports_streaming());

    struct NoStreaming;
    impl LlmProvider for NoStreaming {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<CompletionResponse>> + Send + 'a>,
        > {
            Box::pin(async { unimplemented!() })
        }
        fn supported_models(&self) -> &[&str] {
            &[]
        }
        fn name(&self) -> &str {
            "no-streaming"
        }
    }

    assert!(!NoStreaming.supports_streaming());
    assert_eq!(
        NoStreaming.streaming_capability(),
        StreamingCapability::NONE
    );
}

#[cfg(feature = "cc-provider")]
#[test]
fn anthropic_declares_full_realtime_lifecycle_fidelity() {
    let provider = anthropic_provider_with_builtin_catalog();
    assert_eq!(
        provider.streaming_capability(),
        StreamingCapability::REALTIME_LIFECYCLE
    );
    assert!(provider.supports_streaming());
}

#[cfg(feature = "cc-provider")]
#[test]
fn cc_declares_realtime_text_with_no_lifecycle_or_tool_input_fidelity() {
    let provider = cc_provider_with_dummy_binary();
    let capability = provider.streaming_capability();

    assert!(
        capability.realtime_deltas,
        "CC streams raw incremental subprocess text"
    );
    assert!(
        !capability.lifecycle_events,
        "CC never emits ContentBlockStart/Stop or MessageStart/Stop"
    );
    assert!(
        !capability.tool_input_deltas,
        "CC never emits InputJsonDelta"
    );
    assert!(!capability.buffered_single_delta);
    assert!(provider.supports_streaming());
}
