//! `[[providers]]` admission-cap configuration (#7152).
//!
//! The localhosted launch contract requires a hard admission bound — one
//! running request and at most two bounded waiters — instead of the adaptive
//! concurrency defaults. These tests pin the TOML surface and the
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
