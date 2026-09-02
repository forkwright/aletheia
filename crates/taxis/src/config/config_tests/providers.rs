//! `[[providers]]` admission-cap and token-budget configuration (#7152).
//!
//! The localhosted launch contract requires a hard admission bound — one
//! running request and at most two bounded waiters — instead of the adaptive
//! concurrency defaults, and a token-budget clamp (32768 / 4096 / 8192)
//! wired through `resolve_nous`. These tests pin the TOML surface and the
//! deployment-target-derived defaults.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indexing asserted present by the parsed fixture"
)]

use super::super::*;

fn provider_fixture(extra: &str) -> LlmProviderConfig {
    let toml = format!(
        r#"
[[providers]]
name = "menos-agent"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8189/v1"
deploymentTarget = "localhosted"
models = ["qwen3.8-27b"]
{extra}
"#
    );
    let config: AletheiaConfig = toml::from_str(&toml).expect("provider fixture must parse");
    config.providers[0].clone()
}

#[test]
fn admission_table_parses_camel_case() {
    let entry = provider_fixture(
        r#"
[providers.admission]
mode = "fixed"
maxRunning = 1
maxWaiting = 2
"#,
    );
    let admission = entry.admission.expect("admission table should be present");
    assert_eq!(admission.mode, ProviderAdmissionMode::Fixed);
    assert_eq!(admission.max_running, 1);
    assert_eq!(admission.max_waiting, 2);
}

#[test]
fn localhosted_provider_defaults_to_fixed_one_running_two_waiting() {
    // WHY(#7152): the launch contract's default for deploymentTarget =
    // "localhosted" is a hard cap of 1 running / 2 waiting — the adaptive
    // 10→200 defaults must not be inherited by this endpoint.
    let entry = provider_fixture("");
    assert!(entry.admission.is_none(), "fixture declares no admission");
    let effective = entry.effective_admission();
    assert_eq!(effective.mode, ProviderAdmissionMode::Fixed);
    assert_eq!(effective.max_running, 1);
    assert_eq!(effective.max_waiting, 2);
}

#[test]
fn embedded_provider_defaults_to_fixed_cap() {
    let toml = r#"
[[providers]]
name = "in-process"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8088/v1"
deploymentTarget = "embedded"
models = ["local"]
"#;
    let config: AletheiaConfig = toml::from_str(toml).expect("embedded fixture must parse");
    let effective = config.providers[0].effective_admission();
    assert_eq!(effective.mode, ProviderAdmissionMode::Fixed);
    assert_eq!(effective.max_running, 1);
    assert_eq!(effective.max_waiting, 2);
}

#[test]
fn cloud_provider_defaults_to_adaptive() {
    let toml = r#"
[[providers]]
name = "anthropic-cloud"
providerType = "anthropic"
"#;
    let config: AletheiaConfig = toml::from_str(toml).expect("cloud fixture must parse");
    let effective = config.providers[0].effective_admission();
    assert_eq!(
        effective.mode,
        ProviderAdmissionMode::Adaptive,
        "cloud providers keep the adaptive limiter"
    );
}

#[test]
fn explicit_admission_overrides_deployment_default() {
    let entry = provider_fixture(
        r#"
[providers.admission]
mode = "adaptive"
"#,
    );
    let effective = entry.effective_admission();
    assert_eq!(
        effective.mode,
        ProviderAdmissionMode::Adaptive,
        "an explicit admission table wins over the deployment-target default"
    );
}

#[test]
fn admission_defaults_within_table_are_the_launch_cap() {
    let entry = provider_fixture("[providers.admission]\n");
    let admission = entry.admission.expect("empty admission table is present");
    assert_eq!(admission.mode, ProviderAdmissionMode::Fixed);
    assert_eq!(admission.max_running, 1);
    assert_eq!(admission.max_waiting, 2);
}

#[test]
fn budgets_table_parses_camel_case() {
    let entry = provider_fixture(
        r"
[providers.budgets]
contextTokens = 32768
maxOutputTokens = 4096
bootstrapMaxTokens = 8192
",
    );
    let budgets = entry.budgets.expect("budgets table should be present");
    assert_eq!(budgets.context_tokens, Some(32_768));
    assert_eq!(budgets.max_output_tokens, Some(4_096));
    assert_eq!(budgets.bootstrap_max_tokens, Some(8_192));
}

#[test]
fn budgets_fields_are_individually_optional() {
    let entry = provider_fixture(
        r"
[providers.budgets]
contextTokens = 32768
",
    );
    let budgets = entry.budgets.expect("budgets table should be present");
    assert_eq!(budgets.context_tokens, Some(32_768));
    assert_eq!(budgets.max_output_tokens, None);
    assert_eq!(budgets.bootstrap_max_tokens, None);
}

#[test]
fn snake_case_budget_keys_are_rejected() {
    let toml = r#"
[[providers]]
name = "menos-agent"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8189/v1"
deploymentTarget = "localhosted"
models = ["qwen3.8-27b"]

[providers.budgets]
context_tokens = 32768
"#;
    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("snake_case budget keys should be rejected");
    assert!(
        err.to_string().contains("unknown field"),
        "deny_unknown_fields should reject snake_case keys: {err}"
    );
}

/// The launch-contract budget clamp (#7152): an agent whose primary route
/// resolves to a provider with declared budgets gets its resolved token
/// limits clamped to that provider's declaration — wired through the
/// existing `resolve_nous` budget machinery, not a parallel system.
mod budget_clamp {
    use std::fmt::Write as _;

    use super::super::super::*;

    fn config_with_budgeted_provider(agent_provider: Option<&str>) -> AletheiaConfig {
        let mut toml = String::from(
            r#"
[[providers]]
name = "menos-agent"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8189/v1"
deploymentTarget = "localhosted"
models = ["qwen3.8-27b"]

[providers.budgets]
contextTokens = 32768
maxOutputTokens = 4096
bootstrapMaxTokens = 8192

[agents.defaults]
contextTokens = 200000
maxOutputTokens = 16384
bootstrapMaxTokens = 40000

[[agents.list]]
id = "primary"
workspace = "instance/nous/primary"
"#,
        );
        match agent_provider {
            Some(provider) => {
                let _ = write!(
                    toml,
                    "[agents.list.model]\nprimary = {{ model = \"qwen3.8-27b\", provider = \"{provider}\" }}\n"
                );
            }
            None => {
                toml.push_str("[agents.list.model]\nprimary = \"qwen3.8-27b\"\n");
            }
        }
        toml::from_str(&toml).expect("budget clamp fixture must parse")
    }

    #[test]
    fn named_provider_budgets_clamp_resolved_limits() {
        let config = config_with_budgeted_provider(Some("menos-agent"));
        let resolved = resolve_nous(&config, "primary");
        assert_eq!(resolved.limits.context_tokens, 32_768);
        assert_eq!(resolved.limits.max_output_tokens, 4_096);
        assert_eq!(resolved.limits.bootstrap_max_tokens, 8_192);
    }

    #[test]
    fn model_claim_budgets_clamp_resolved_limits() {
        // No explicit provider on the route: the first provider claiming the
        // model in list order (the registry's routing rule) supplies the clamp.
        let config = config_with_budgeted_provider(None);
        let resolved = resolve_nous(&config, "primary");
        assert_eq!(resolved.limits.context_tokens, 32_768);
        assert_eq!(resolved.limits.max_output_tokens, 4_096);
        assert_eq!(resolved.limits.bootstrap_max_tokens, 8_192);
    }

    #[test]
    fn budgets_never_inflate_smaller_agent_limits() {
        let mut config = config_with_budgeted_provider(Some("menos-agent"));
        config.agents.defaults.model_defaults.max_output_tokens = 2_048;
        let resolved = resolve_nous(&config, "primary");
        assert_eq!(
            resolved.limits.max_output_tokens, 2_048,
            "clamp is min(), never a raise"
        );
    }

    #[test]
    fn unbudgeted_provider_leaves_limits_unchanged() {
        let mut config = config_with_budgeted_provider(Some("menos-agent"));
        config.providers[0].budgets = None;
        let resolved = resolve_nous(&config, "primary");
        assert_eq!(resolved.limits.context_tokens, 200_000);
        assert_eq!(resolved.limits.max_output_tokens, 16_384);
        assert_eq!(resolved.limits.bootstrap_max_tokens, 40_000);
    }

    #[test]
    fn other_agents_routes_are_not_clamped() {
        // An agent routed elsewhere must not inherit the local provider's clamp.
        let config = config_with_budgeted_provider(Some("menos-agent"));
        let resolved = resolve_nous(&config, "unrelated-agent");
        assert_eq!(resolved.limits.context_tokens, 200_000);
        assert_eq!(resolved.limits.max_output_tokens, 16_384);
    }
}

#[test]
fn snake_case_admission_keys_are_rejected() {
    let toml = r#"
[[providers]]
name = "menos-agent"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8189/v1"
deploymentTarget = "localhosted"
models = ["qwen3.8-27b"]

[providers.admission]
max_running = 1
"#;
    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("snake_case admission keys should be rejected");
    assert!(
        err.to_string().contains("unknown field"),
        "deny_unknown_fields should reject snake_case keys: {err}"
    );
}
