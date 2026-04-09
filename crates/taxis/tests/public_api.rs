//! Integration tests for aletheia-taxis's public config-type API.
//!
//! WHY: taxis had zero `crates/taxis/tests/` integration tests prior to this
//! (tracked in aletheia#2814). taxis is the configuration and workspace-layout
//! layer -- every Aletheia subsystem depends on it. This binary covers the
//! pure in-memory config surface (defaults, serde, [`resolve_nous`], redaction,
//! workspace-schema builder).
//!
//! Filesystem tests live in sibling binaries:
//!   - `public_api_loader.rs` -- TOML loading, env interpolation, roundtrip
//!   - `public_api_oikos.rs`  -- Oikos paths, validation, cascade, preflight

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "serde_json::Value indexing on known-present keys in redaction assertions"
)]

use aletheia_koina::secret::SecretString;
use aletheia_taxis::config::{
    AgencyLevel, AletheiaConfig, ChannelBinding, EgressPolicy, ModelPricing, ModelSpec,
    NousDefinition, SandboxEnforcementMode, SignalAccountConfig, resolve_nous,
};
use aletheia_taxis::redact::redact;
use aletheia_taxis::workspace_schema::{RequirementKind, WorkspaceRequirement, WorkspaceSchema};

// ─── Default config values ──────────────────────────────────────────────

#[test]
fn default_gateway_port_is_18789() {
    // INVARIANT: downstream callers hard-code 18789 in their own fixtures;
    // changing this default is a breaking change.
    assert_eq!(AletheiaConfig::default().gateway.port, 18789);
}

#[test]
fn default_gateway_binds_localhost() {
    assert_eq!(AletheiaConfig::default().gateway.bind, "localhost");
}

#[test]
fn default_gateway_auth_mode_is_token() {
    assert_eq!(AletheiaConfig::default().gateway.auth.mode, "token");
}

#[test]
fn default_agency_level_is_standard() {
    assert_eq!(
        AletheiaConfig::default().agents.defaults.agency,
        AgencyLevel::Standard
    );
}

#[test]
fn default_agent_list_is_empty() {
    assert!(AletheiaConfig::default().agents.list.is_empty());
}

#[test]
fn default_embedding_provider_is_candle_with_384_dimensions() {
    let c = AletheiaConfig::default();
    assert_eq!(c.embedding.provider, "candle");
    assert_eq!(c.embedding.dimension, 384);
}

#[test]
fn default_signal_channel_is_enabled_with_no_accounts() {
    let c = AletheiaConfig::default();
    assert!(c.channels.signal.enabled);
    assert!(c.channels.signal.accounts.is_empty());
}

// ─── resolve_nous: defaults, overrides, agency levels ───────────────────

#[test]
fn resolve_nous_unknown_agent_falls_back_to_defaults() {
    let resolved = resolve_nous(&AletheiaConfig::default(), "unknown");
    assert_eq!(&*resolved.id, "unknown");
    assert_eq!(&*resolved.model.primary, "claude-sonnet-4-6");
    assert!(resolved.domains.is_empty());
}

#[test]
fn resolve_nous_agent_model_override_replaces_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "syn".to_owned(),
        name: Some("Synthetic".to_owned()),
        model: Some(ModelSpec {
            primary: "claude-opus-4-6".to_owned(),
            fallbacks: vec!["claude-sonnet-4-6".to_owned()],
            retries_before_fallback: 1,
        }),
        workspace: "/tmp/syn".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: vec!["code".to_owned()],
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(&*resolved.model.primary, "claude-opus-4-6");
    assert_eq!(resolved.model.retries_before_fallback, 1);
    assert_eq!(resolved.domains, vec!["code"]);
}

#[test]
fn resolve_nous_unrestricted_agency_sets_10k_tool_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "free".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/free".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "free");
    assert_eq!(resolved.capabilities.agency, AgencyLevel::Unrestricted);
    assert_eq!(resolved.capabilities.max_tool_iterations, 10_000);
}

#[test]
fn resolve_nous_restricted_agency_sets_50_tool_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "safe".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/safe".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Restricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "safe");
    assert_eq!(resolved.capabilities.max_tool_iterations, 50);
}

// ─── Serde round-trips (no filesystem) ──────────────────────────────────

#[test]
fn full_config_json_roundtrip_preserves_defaults() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.gateway.port, config.gateway.port);
    assert_eq!(back.embedding.provider, config.embedding.provider);
}

#[test]
fn full_config_toml_roundtrip_preserves_overrides() {
    let mut config = AletheiaConfig::default();
    config.gateway.port = 9001;
    config.embedding.dimension = 1536;

    let toml_str = toml::to_string(&config).expect("serialize toml");
    let back: AletheiaConfig = toml::from_str(&toml_str).expect("deserialize toml");
    assert_eq!(back.gateway.port, 9001);
    assert_eq!(back.embedding.dimension, 1536);
}

#[test]
fn channel_binding_json_uses_camel_case_nous_id() {
    let json = r#"{"channel":"signal","source":"*","nousId":"syn"}"#;
    let binding: ChannelBinding = serde_json::from_str(json).expect("parse");
    assert_eq!(binding.channel, "signal");
    assert_eq!(binding.source, "*");
    assert_eq!(binding.nous_id, "syn");
    assert_eq!(binding.session_key, "{source}", "session key default");
}

#[test]
fn model_pricing_parses_float_costs_from_json() {
    let json = r#"{"inputCostPerMtok":15.0,"outputCostPerMtok":75.0}"#;
    let p: ModelPricing = serde_json::from_str(json).expect("parse pricing");
    assert!((p.input_cost_per_mtok - 15.0).abs() < f64::EPSILON);
    assert!((p.output_cost_per_mtok - 75.0).abs() < f64::EPSILON);
}

#[test]
fn signal_account_defaults_to_port_8080_enabled() {
    let a = SignalAccountConfig::default();
    assert_eq!(a.http_port, 8080);
    assert!(a.enabled);
}

#[test]
fn egress_policy_default_is_allow() {
    assert_eq!(EgressPolicy::default(), EgressPolicy::Allow);
}

#[test]
fn sandbox_enforcement_mode_serializes_lowercase() {
    let json = serde_json::to_string(&SandboxEnforcementMode::Enforcing).expect("serialize");
    assert_eq!(json, "\"enforcing\"");
}

// ─── Config redaction ───────────────────────────────────────────────────

#[test]
fn redact_scrubs_gateway_signing_key_string() {
    let mut config = AletheiaConfig::default();
    config.gateway.auth.signing_key = Some(SecretString::from("super-secret-literal-0xbeef"));

    let redacted = redact(&config);
    // INVARIANT: raw secret literal must never appear in redacted output.
    assert!(
        !redacted.to_string().contains("super-secret-literal-0xbeef"),
        "redact() must scrub the raw secret"
    );
}

#[test]
fn redact_preserves_non_sensitive_gateway_port() {
    let redacted = redact(&AletheiaConfig::default());
    assert_eq!(redacted["gateway"]["port"], 18789);
}

#[test]
fn redact_preserves_non_sensitive_embedding_provider() {
    let redacted = redact(&AletheiaConfig::default());
    assert_eq!(redacted["embedding"]["provider"], "candle");
}

#[test]
fn redact_scrubs_tls_key_and_cert_paths() {
    let mut config = AletheiaConfig::default();
    config.gateway.tls.key_path = Some("/etc/ssl/redact-probe-key.pem".to_owned());
    config.gateway.tls.cert_path = Some("/etc/ssl/redact-probe-cert.pem".to_owned());

    let redacted = redact(&config).to_string();
    assert!(!redacted.contains("redact-probe-key.pem"), "key path leaked");
    assert!(!redacted.contains("redact-probe-cert.pem"), "cert path leaked");
}

// ─── WorkspaceSchema builder API ────────────────────────────────────────

#[test]
fn workspace_schema_new_constructs_empty_schema() {
    // NOTE: WorkspaceSchema::validate is pub(crate); only the builder API is
    // exercised here. File an aletheia issue if downstream consumers need
    // programmatic workspace validation from outside the taxis crate.
    let _schema = WorkspaceSchema::new();
}

#[test]
fn workspace_schema_standard_constructs_default_schema() {
    let _schema = WorkspaceSchema::standard();
}

#[test]
fn workspace_schema_require_chains_multiple_requirements() {
    let _schema = WorkspaceSchema::new()
        .require(WorkspaceRequirement {
            path: "SOUL.md",
            kind: RequirementKind::File,
        })
        .require(WorkspaceRequirement {
            path: "bootstrap",
            kind: RequirementKind::Directory,
        });
}
