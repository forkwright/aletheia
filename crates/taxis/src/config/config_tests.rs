#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: map/vec indexing with keys/indices asserted present by surrounding context"
)]

use std::sync::Arc;

use super::*;

#[test]
#[expect(clippy::too_many_lines, reason = "full config default validation")]
fn defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "default context tokens should be 200k"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.max_output_tokens, 16_384,
        "default max output tokens should be 16384"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.bootstrap_max_tokens, 40_000,
        "default bootstrap max tokens should be 40k"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.model.primary, "claude-sonnet-4-6",
        "default primary model should be sonnet"
    );
    assert!(
        !config.agents.defaults.model_defaults.thinking_enabled,
        "thinking should be disabled by default"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.thinking_budget, 10_000,
        "default thinking budget should be 10k"
    );
    assert_eq!(
        config.agents.defaults.max_tool_iterations, 200,
        "default max tool iterations should be 200"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "default gateway port should be 18789"
    );
    assert_eq!(
        config.gateway.bind, "localhost",
        "default gateway bind should be localhost"
    );
    assert_eq!(
        config.gateway.auth.mode, "token",
        "default auth mode should be token"
    );
    assert!(
        !config.gateway.tls.enabled,
        "tls should be disabled by default"
    );
    assert!(
        config.gateway.tls.cert_path.is_none(),
        "tls cert path should be absent by default"
    );
    assert!(
        config.gateway.cors.allowed_origins.is_empty(),
        "cors allowed origins should be empty by default"
    );
    assert_eq!(
        config.gateway.cors.max_age_secs, 3600,
        "default cors max age should be 3600 seconds"
    );
    assert_eq!(
        config.gateway.body_limit.max_bytes, 1_048_576,
        "default body limit should be 1 MiB"
    );
    assert!(
        !config.gateway.csrf.enabled,
        "csrf should be disabled by default (#1690)"
    );
    assert_eq!(
        config.gateway.csrf.header_name, "x-requested-with",
        "default csrf header name should be x-requested-with"
    );
    assert_eq!(
        config.gateway.csrf.header_value, "aletheia",
        "default csrf header value should be aletheia"
    );
    assert!(
        !config.gateway.rate_limit.enabled,
        "rate limiting should be disabled by default"
    );
    assert_eq!(
        config.gateway.rate_limit.requests_per_minute, 60,
        "default rate limit should be 60 rpm"
    );
    assert!(
        config.channels.signal.enabled,
        "signal channel should be enabled by default"
    );
    assert!(
        config.channels.signal.accounts.is_empty(),
        "signal accounts should be empty by default"
    );
    assert!(
        config.bindings.is_empty(),
        "bindings should be empty by default"
    );
    assert_eq!(
        config.embedding.provider, "candle",
        "default embedding provider should be candle"
    );
    assert!(
        config.embedding.model.is_none(),
        "embedding model should be unset by default"
    );
    assert_eq!(
        config.embedding.dimension, 384,
        "default embedding dimension should be 384"
    );
    assert!(
        config.maintenance.trace_rotation.enabled,
        "trace rotation should be enabled by default"
    );
    assert_eq!(
        config.maintenance.trace_rotation.max_age_days, 14,
        "default trace rotation max age should be 14 days"
    );
    assert!(
        config.maintenance.drift_detection.enabled,
        "drift detection should be enabled by default"
    );
    assert!(
        config.maintenance.db_monitoring.enabled,
        "db monitoring should be enabled by default"
    );
    assert_eq!(
        config.maintenance.db_monitoring.warn_threshold_mb, 100,
        "default db monitoring warn threshold should be 100 MiB"
    );
    assert!(
        !config.maintenance.retention.enabled,
        "retention should be disabled by default"
    );
    assert!(
        config.pricing.is_empty(),
        "pricing map should be empty by default"
    );
    assert!(
        config.mcp.rate_limit.enabled,
        "mcp rate limit should be enabled by default"
    );
    assert_eq!(
        config.mcp.rate_limit.message_requests_per_minute, 60,
        "default mcp message rate limit should be 60 rpm"
    );
    assert_eq!(
        config.mcp.rate_limit.read_requests_per_minute, 300,
        "default mcp read rate limit should be 300 rpm"
    );
    assert!(
        !config.training.enabled,
        "training capture should be disabled by default"
    );
    assert_eq!(
        config.training.path, "data/training",
        "default training path should be data/training"
    );
}

#[test]
fn serde_roundtrip() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.agents.defaults.model_defaults.context_tokens, 200_000,
        "context tokens should survive serde roundtrip"
    );
    assert_eq!(
        back.gateway.port, 18789,
        "gateway port should survive serde roundtrip"
    );
    assert!(
        back.channels.signal.enabled,
        "signal channel enabled should survive serde roundtrip"
    );
    assert_eq!(
        back.embedding.provider, "candle",
        "embedding provider should survive serde roundtrip"
    );
    assert_eq!(
        back.embedding.dimension, 384,
        "embedding dimension should survive serde roundtrip"
    );
}

#[test]
fn minimal_yaml_parses() {
    let yaml = r#"{"agents": {"list": []}}"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse minimal");
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "minimal yaml should use default context tokens"
    );
    assert!(
        config.agents.list.is_empty(),
        "minimal yaml agent list should be empty"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "minimal yaml should use default gateway port"
    );
}

#[test]
fn camel_case_compat() {
    let yaml = r#"{
        "agents": {
            "defaults": {
                "contextTokens": 100000,
                "maxOutputTokens": 8192,
                "bootstrapMaxTokens": 20000
            },
            "list": []
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse camelCase");
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 100_000,
        "camelCase contextTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.max_output_tokens, 8192,
        "camelCase maxOutputTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.bootstrap_max_tokens, 20_000,
        "camelCase bootstrapMaxTokens should be accepted"
    );
}

#[test]
fn resolve_uses_defaults_for_unknown_agent() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "unknown-agent");
    assert_eq!(
        resolved.id.as_ref(), "unknown-agent",
        "resolved id should match requested agent id"
    );
    assert_eq!(
        resolved.model.primary, Arc::<str>::from("claude-sonnet-4-6"),
        "unknown agent should use default primary model"
    );
    assert_eq!(
        resolved.limits.context_tokens, 200_000,
        "unknown agent should use default context tokens"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "unknown agent should inherit thinking disabled"
    );
    assert_eq!(
        resolved.workspace, "instance/nous/unknown-agent",
        "unknown agent workspace should use default path"
    );
    assert!(
        resolved.domains.is_empty(),
        "unknown agent should have no domains"
    );
}

#[test]
fn resolve_merges_agent_overrides() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "syn".to_owned(),
        name: Some("Synthetic".to_owned()),
        model: Some(ModelSpec {
            primary: "claude-opus-4-6".to_owned(),
            fallbacks: vec!["claude-sonnet-4-6".to_owned()],
            retries_before_fallback: 2,
        }),
        workspace: "/home/user/nous/syn".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: vec!["code".to_owned()],
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(
        resolved.model.primary, Arc::<str>::from("claude-opus-4-6"),
        "agent override should replace primary model"
    );
    assert_eq!(
        resolved.model.fallbacks,
        vec![Arc::<str>::from("claude-sonnet-4-6")],
        "agent override should set fallback model"
    );
    assert_eq!(
        resolved.name,
        Some("Synthetic".to_owned()),
        "agent name override should be applied"
    );
    assert_eq!(
        resolved.workspace, "/home/user/nous/syn",
        "agent workspace override should be applied"
    );
    assert_eq!(
        resolved.domains,
        vec!["code"],
        "agent domain override should be applied"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "thinking should remain disabled when not overridden"
    );
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "agency should default to standard when not overridden"
    );
}

#[test]
fn resolve_merges_allowed_roots() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.allowed_roots = vec!["/shared".to_owned()];
    config.agents.list.push(NousDefinition {
        id: "syn".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/syn".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: vec!["/extra".to_owned(), "/shared".to_owned()],
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(
        resolved.allowed_roots,
        vec!["/shared", "/extra"],
        "allowed roots should merge defaults and agent-specific roots deduped"
    );
}

#[test]
fn resolve_thinking_override() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "thinker".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/thinker".to_owned(),
        thinking_enabled: Some(true),
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "thinker");
    assert!(
        resolved.capabilities.thinking_enabled,
        "thinking should be enabled when agent overrides to true"
    );
}

#[test]
fn signal_account_defaults() {
    let account = SignalAccountConfig::default();
    assert!(
        account.enabled,
        "signal account should be enabled by default"
    );
    assert_eq!(
        account.http_host, "localhost",
        "signal account default host should be localhost"
    );
    assert_eq!(
        account.http_port, 8080,
        "signal account default port should be 8080"
    );
    assert!(
        account.auto_start,
        "signal account should auto-start by default"
    );
}

#[test]
fn channel_binding_serde_roundtrip() {
    let json = r#"{
        "channel": "signal",
        "source": "+1234567890",
        "nousId": "syn"
    }"#;
    let binding: ChannelBinding = serde_json::from_str(json).expect("parse binding");
    assert_eq!(
        binding.channel, "signal",
        "channel field should deserialize correctly"
    );
    assert_eq!(
        binding.source, "+1234567890",
        "source field should deserialize correctly"
    );
    assert_eq!(
        binding.nous_id, "syn",
        "nous_id field should deserialize correctly"
    );
    assert_eq!(
        binding.session_key, "{source}",
        "session_key should default to source pattern"
    );
}

#[test]
fn embedding_override_from_json() {
    let json = r#"{
        "embedding": {
            "provider": "candle",
            "model": "BAAI/bge-small-en-v1.5",
            "dimension": 512
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse embedding");
    assert_eq!(
        config.embedding.provider, "candle",
        "embedding provider should be parsed from json"
    );
    assert_eq!(
        config.embedding.model,
        Some("BAAI/bge-small-en-v1.5".to_owned()),
        "embedding model should be parsed from json"
    );
    assert_eq!(
        config.embedding.dimension, 512,
        "embedding dimension override should be parsed from json"
    );
}

#[test]
fn bindings_in_config_default_empty() {
    let config = AletheiaConfig::default();
    assert!(
        config.bindings.is_empty(),
        "bindings should be empty in default config"
    );
}

#[test]
fn pricing_defaults_empty() {
    let config = AletheiaConfig::default();
    assert!(
        config.pricing.is_empty(),
        "pricing map should be empty in default config"
    );
}

#[test]
fn pricing_from_json() {
    let json = r#"{
        "pricing": {
            "claude-opus-4-6": {
                "inputCostPerMtok": 15.0,
                "outputCostPerMtok": 75.0
            },
            "claude-sonnet-4-6": {
                "inputCostPerMtok": 3.0,
                "outputCostPerMtok": 15.0
            }
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse pricing");
    assert_eq!(
        config.pricing.len(),
        2,
        "pricing map should contain two entries"
    );
    let opus = &config.pricing["claude-opus-4-6"];
    assert!(
        (opus.input_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "opus input cost should be 15.0"
    );
    assert!(
        (opus.output_cost_per_mtok - 75.0).abs() < f64::EPSILON,
        "opus output cost should be 75.0"
    );
    let sonnet = &config.pricing["claude-sonnet-4-6"];
    assert!(
        (sonnet.input_cost_per_mtok - 3.0).abs() < f64::EPSILON,
        "sonnet input cost should be 3.0"
    );
    assert!(
        (sonnet.output_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "sonnet output cost should be 15.0"
    );
}

#[test]
fn agency_default_is_standard() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Standard,
        "default agency level should be standard"
    );
}

#[test]
fn agency_serde_roundtrip() {
    let json = serde_json::to_string(&AgencyLevel::Unrestricted).expect("serialize");
    assert_eq!(
        json, "\"unrestricted\"",
        "unrestricted agency should serialize to string 'unrestricted'"
    );
    let back: AgencyLevel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        AgencyLevel::Unrestricted,
        "unrestricted agency should roundtrip through serde"
    );

    let json = serde_json::to_string(&AgencyLevel::Restricted).expect("serialize");
    assert_eq!(
        json, "\"restricted\"",
        "restricted agency should serialize to string 'restricted'"
    );
}

#[test]
fn resolve_agency_inherits_global_default() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "any");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "global default agency should be standard"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 200,
        "standard agency should use default max tool iterations"
    );
}

#[test]
fn resolve_agency_unrestricted_sets_high_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "free".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/free".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "free");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "agent agency override should be unrestricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "unrestricted agency should set max tool iterations to 10k"
    );
}

#[test]
fn resolve_agency_restricted_uses_old_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "safe".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/safe".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Restricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "safe");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Restricted,
        "agent agency override should be restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 50,
        "restricted agency should use low max tool iterations"
    );
}

#[test]
fn resolve_agency_per_agent_overrides_global() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.agency = AgencyLevel::Restricted;
    config.agents.list.push(NousDefinition {
        id: "override".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/override".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });

    let resolved = resolve_nous(&config, "override");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "per-agent unrestricted should override global restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "per-agent unrestricted override should set iterations to 10k"
    );

    let other = resolve_nous(&config, "other");
    assert_eq!(
        other.capabilities.agency,
        AgencyLevel::Restricted,
        "agent without override should use global restricted agency"
    );
    assert_eq!(
        other.capabilities.max_tool_iterations, 50,
        "agent using global restricted should get low max tool iterations"
    );
}

#[test]
fn agency_from_json() {
    let json = r#"{
        "agents": {
            "defaults": {
                "agency": "unrestricted"
            },
            "list": [{
                "id": "restricted-agent",
                "workspace": "/tmp/ws",
                "agency": "restricted"
            }]
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse agency");
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Unrestricted,
        "global agency override from json should be unrestricted"
    );
    assert_eq!(
        config.agents.list[0].agency,
        Some(AgencyLevel::Restricted),
        "per-agent restricted override from json should be applied"
    );
}

// ── Wave 1: parameterized defaults ───────────────────────────────────────────

#[test]
fn timeouts_default_matches_koina_const() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.timeouts.llm_call_secs,
        koina::defaults::TIMEOUT_SECONDS,
        "default llm_call_secs must equal koina::defaults::TIMEOUT_SECONDS"
    );
    assert_eq!(
        config.timeouts.llm_call_secs, 300,
        "default llm_call_secs must be 300 seconds"
    );
}

#[test]
fn capacity_defaults_match_koina_consts() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.capacity.max_tool_output_bytes,
        koina::defaults::MAX_OUTPUT_BYTES,
        "default max_tool_output_bytes must equal koina::defaults::MAX_OUTPUT_BYTES"
    );
    assert_eq!(
        config.capacity.max_tool_output_bytes,
        51_200,
        "default max_tool_output_bytes must be 51200 (50 KiB)"
    );
    assert_eq!(
        config.capacity.opus_context_tokens,
        koina::defaults::OPUS_CONTEXT_TOKENS,
        "default opus_context_tokens must equal koina::defaults::OPUS_CONTEXT_TOKENS"
    );
    assert_eq!(
        config.capacity.opus_context_tokens, 1_000_000,
        "default opus_context_tokens must be 1 000 000"
    );
}

#[test]
fn retry_defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.retry.max_attempts, 3,
        "default max_attempts must be 3"
    );
    assert_eq!(
        config.retry.backoff_base_ms, 1_000,
        "default backoff_base_ms must be 1000"
    );
    assert_eq!(
        config.retry.backoff_max_ms, 30_000,
        "default backoff_max_ms must be 30 000"
    );
    assert!(
        config.retry.backoff_max_ms >= config.retry.backoff_base_ms,
        "backoff_max_ms must be >= backoff_base_ms"
    );
}

#[test]
fn timeouts_override_from_json() {
    let json = r#"{"timeouts": {"llmCallSecs": 600}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse timeouts override");
    assert_eq!(
        config.timeouts.llm_call_secs, 600,
        "llm_call_secs override from json should take effect"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "unrelated gateway port should remain at default"
    );
}

#[test]
fn capacity_override_from_json() {
    let json = r#"{"capacity": {"maxToolOutputBytes": 102400, "opusContextTokens": 500000}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse capacity override");
    assert_eq!(
        config.capacity.max_tool_output_bytes, 102_400,
        "max_tool_output_bytes override from json should take effect"
    );
    assert_eq!(
        config.capacity.opus_context_tokens, 500_000,
        "opus_context_tokens override from json should take effect"
    );
}

#[test]
fn retry_override_from_json() {
    let json = r#"{"retry": {"maxAttempts": 5, "backoffBaseMs": 2000, "backoffMaxMs": 60000}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse retry override");
    assert_eq!(
        config.retry.max_attempts, 5,
        "max_attempts override from json should take effect"
    );
    assert_eq!(
        config.retry.backoff_base_ms, 2_000,
        "backoff_base_ms override from json should take effect"
    );
    assert_eq!(
        config.retry.backoff_max_ms, 60_000,
        "backoff_max_ms override from json should take effect"
    );
}

#[test]
fn new_sections_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.timeouts.llm_call_secs = 120;
    config.capacity.max_tool_output_bytes = 8192;
    config.capacity.opus_context_tokens = 500_000;
    config.retry.max_attempts = 1;
    config.retry.backoff_base_ms = 500;
    config.retry.backoff_max_ms = 5_000;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.timeouts.llm_call_secs, 120,
        "llm_call_secs should survive serde roundtrip"
    );
    assert_eq!(
        back.capacity.max_tool_output_bytes, 8192,
        "max_tool_output_bytes should survive serde roundtrip"
    );
    assert_eq!(
        back.capacity.opus_context_tokens, 500_000,
        "opus_context_tokens should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.max_attempts, 1,
        "max_attempts should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.backoff_base_ms, 500,
        "backoff_base_ms should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.backoff_max_ms, 5_000,
        "backoff_max_ms should survive serde roundtrip"
    );
}

// ─── Wave 0 (#2306): config schema for 190 behavioral constants ──────────────

#[test]
fn deployment_defaults_match_original_constants() {
    let nb = NousBehaviorConfig::default();
    // nous::actor::DEGRADED_PANIC_THRESHOLD = 5
    assert_eq!(nb.degraded_panic_threshold, 5, "degraded_panic_threshold");
    // nous::actor::DEGRADED_WINDOW = 600s
    assert_eq!(nb.degraded_window_secs, 600, "degraded_window_secs");
    // nous::actor::INBOX_RECV_TIMEOUT = 30s
    assert_eq!(nb.inbox_recv_timeout_secs, 30, "inbox_recv_timeout_secs");
    // nous::actor::CONSECUTIVE_TIMEOUT_WARN_THRESHOLD = 3
    assert_eq!(nb.consecutive_timeout_warn_threshold, 3, "consecutive_timeout_warn_threshold");
    assert_eq!(nb.inbox_capacity, 32, "inbox_capacity");
    assert_eq!(nb.max_spawned_tasks, 8, "max_spawned_tasks");
    assert_eq!(nb.max_sessions, 1_000, "max_sessions");
    // nous::tasks::gc::DEFAULT_GC_INTERVAL = 300s
    assert_eq!(nb.gc_interval_secs, 300, "gc_interval_secs");
    // nous::manager::DEAD_THRESHOLD = 3
    assert_eq!(nb.manager_dead_threshold, 3, "manager_dead_threshold");
    // nous::manager::MAX_RESTART_BACKOFF = 300s
    assert_eq!(nb.manager_max_restart_backoff_secs, 300, "manager_max_restart_backoff_secs");
    // nous::manager::RESTART_DRAIN_TIMEOUT = 30s
    assert_eq!(nb.manager_restart_drain_timeout_secs, 30, "manager_restart_drain_timeout_secs");
    // nous::manager::RESTART_DECAY_WINDOW = 3600s
    assert_eq!(nb.manager_restart_decay_window_secs, 3_600, "manager_restart_decay_window_secs");
    // nous::manager::DEFAULT_HEALTH_INTERVAL = 30s
    assert_eq!(nb.manager_health_interval_secs, 30, "manager_health_interval_secs");
    // nous::manager::DEFAULT_PING_TIMEOUT = 5s
    assert_eq!(nb.manager_ping_timeout_secs, 5, "manager_ping_timeout_secs");
    // nous::pipeline::DEFAULT_LOOP_WINDOW = 50
    assert_eq!(nb.loop_detection_window, 50, "loop_detection_window");
    // nous::pipeline::CYCLE_DETECTION_MAX_LEN = 10
    assert_eq!(nb.cycle_detection_max_len, 10, "cycle_detection_max_len");
    // nous::self_audit::DEFAULT_EVENT_THRESHOLD = 50
    assert_eq!(nb.self_audit_event_threshold, 50, "self_audit_event_threshold");

    let kc = KnowledgeConfig::default();
    // episteme::conflict::MAX_LLM_CALLS_PER_FACT = 3
    assert_eq!(kc.conflict_max_llm_calls_per_fact, 3, "conflict_max_llm_calls_per_fact");
    // episteme::conflict::INTRA_BATCH_DEDUP_THRESHOLD = 0.95
    assert!((kc.conflict_intra_batch_dedup_threshold - 0.95).abs() < f64::EPSILON, "conflict_intra_batch_dedup_threshold");
    // episteme::conflict::CANDIDATE_DISTANCE_THRESHOLD = 0.28
    assert!((kc.conflict_candidate_distance_threshold - 0.28).abs() < f64::EPSILON, "conflict_candidate_distance_threshold");
    // episteme::conflict::MAX_CANDIDATES = 5
    assert_eq!(kc.conflict_max_candidates, 5, "conflict_max_candidates");
    // episteme::decay::REINFORCEMENT_BOOST = 0.02
    assert!((kc.decay_reinforcement_boost - 0.02).abs() < f64::EPSILON, "decay_reinforcement_boost");
    // episteme::decay::MAX_REINFORCEMENT_BONUS = 1.0
    assert!((kc.decay_max_reinforcement_bonus - 1.0).abs() < f64::EPSILON, "decay_max_reinforcement_bonus");
    // episteme::decay::CROSS_AGENT_BONUS_PER_AGENT = 0.15
    assert!((kc.decay_cross_agent_bonus_per_agent - 0.15).abs() < f64::EPSILON, "decay_cross_agent_bonus_per_agent");
    // episteme::decay::MAX_CROSS_AGENT_MULTIPLIER = 1.75
    assert!((kc.decay_max_cross_agent_multiplier - 1.75).abs() < f64::EPSILON, "decay_max_cross_agent_multiplier");
    assert!((kc.extraction_confidence_threshold - 0.3).abs() < f64::EPSILON, "extraction_confidence_threshold");
    assert_eq!(kc.extraction_min_fact_length, 10, "extraction_min_fact_length");
    assert_eq!(kc.extraction_max_fact_length, 500, "extraction_max_fact_length");
    // episteme::ops_facts::MIN_TOOL_CALLS = 5
    assert_eq!(kc.instinct_min_tool_calls, 5, "instinct_min_tool_calls");

    let pb = ProviderBehaviorConfig::default();
    // hermeneus::anthropic::client::NON_STREAMING_TIMEOUT = 120s
    assert_eq!(pb.non_streaming_timeout_secs, 120, "non_streaming_timeout_secs");
    // hermeneus::anthropic::error::SSE_DEFAULT_RETRY_MS = 1000
    assert_eq!(pb.sse_default_retry_ms, 1_000, "sse_default_retry_ms");
    // hermeneus::concurrency::DEFAULT_EWMA_ALPHA = 0.8
    assert!((pb.concurrency_ewma_alpha - 0.8).abs() < f64::EPSILON, "concurrency_ewma_alpha");
    // hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS = 30.0
    assert!((pb.concurrency_latency_threshold_secs - 30.0).abs() < f64::EPSILON, "concurrency_latency_threshold_secs");
    // hermeneus::complexity::DEFAULT_LOW_THRESHOLD = 30
    assert_eq!(pb.complexity_low_threshold, 30, "complexity_low_threshold");
    // hermeneus::complexity::DEFAULT_HIGH_THRESHOLD = 70
    assert_eq!(pb.complexity_high_threshold, 70, "complexity_high_threshold");

    let al = ApiLimitsConfig::default();
    // pylon::handlers::sessions::MAX_SESSION_NAME_LEN = 255
    assert_eq!(al.max_session_name_len, 255, "max_session_name_len");
    // pylon::handlers::sessions::MAX_IDENTIFIER_BYTES = 256
    assert_eq!(al.max_identifier_bytes, 256, "max_identifier_bytes");
    // pylon::handlers::sessions::MAX_HISTORY_LIMIT = 1000
    assert_eq!(al.max_history_limit, 1_000, "max_history_limit");
    // pylon::handlers::sessions::DEFAULT_HISTORY_LIMIT = 50
    assert_eq!(al.default_history_limit, 50, "default_history_limit");
    // pylon::handlers::sessions::streaming::MAX_MESSAGE_BYTES = 262144
    assert_eq!(al.max_message_bytes, 262_144, "max_message_bytes");
    // pylon::handlers::knowledge::MAX_FACTS_LIMIT = 1000
    assert_eq!(al.max_facts_limit, 1_000, "max_facts_limit");
    // pylon::handlers::knowledge::MAX_SEARCH_LIMIT = 1000
    assert_eq!(al.max_search_limit, 1_000, "max_search_limit");
    // pylon::handlers::knowledge::bulk_import::MAX_IMPORT_BATCH_SIZE = 1000
    assert_eq!(al.max_import_batch_size, 1_000, "max_import_batch_size");
    // pylon::idempotency::DEFAULT_TTL = 300s
    assert_eq!(al.idempotency_ttl_secs, 300, "idempotency_ttl_secs");
    // pylon::idempotency::DEFAULT_CAPACITY = 10000
    assert_eq!(al.idempotency_capacity, 10_000, "idempotency_capacity");
    assert_eq!(al.idempotency_max_key_length, 64, "idempotency_max_key_length");
    // pylon::handlers::health::CLOCK_SKEW_LEEWAY = 30s
    assert_eq!(al.clock_skew_leeway_secs, 30, "clock_skew_leeway_secs");
    // pylon::handlers::health::EXPIRY_WARNING_THRESHOLD = 3600s
    assert_eq!(al.expiry_warning_threshold_secs, 3_600, "expiry_warning_threshold_secs");

    let db = DaemonBehaviorConfig::default();
    // daemon::watchdog::BACKOFF_BASE = 2s
    assert_eq!(db.watchdog_backoff_base_secs, 2, "watchdog_backoff_base_secs");
    // daemon::watchdog::BACKOFF_CAP = 300s
    assert_eq!(db.watchdog_backoff_cap_secs, 300, "watchdog_backoff_cap_secs");
    // daemon::prosoche::ANOMALY_SAMPLE_SIZE = 15
    assert_eq!(db.prosoche_anomaly_sample_size, 15, "prosoche_anomaly_sample_size");
    // daemon::runner::output::BRIEF_HEAD_LINES = 5
    assert_eq!(db.runner_output_brief_head_lines, 5, "runner_output_brief_head_lines");
    // daemon::runner::output::BRIEF_TAIL_LINES = 3
    assert_eq!(db.runner_output_brief_tail_lines, 3, "runner_output_brief_tail_lines");

    let tl = ToolLimitsConfig::default();
    // organon::builtins::filesystem::MAX_PATTERN_LENGTH = 1000
    assert_eq!(tl.max_pattern_length, 1_000, "max_pattern_length");
    // organon::builtins::filesystem::SUBPROCESS_TIMEOUT = 60s
    assert_eq!(tl.subprocess_timeout_secs, 60, "subprocess_timeout_secs");
    // organon::builtins::workspace::MAX_WRITE_BYTES = 10 MiB
    assert_eq!(tl.max_write_bytes, 10_485_760, "max_write_bytes");
    // organon::builtins::workspace::MAX_READ_BYTES = 50 MiB
    assert_eq!(tl.max_read_bytes, 52_428_800, "max_read_bytes");
    // organon::builtins::workspace::MAX_COMMAND_LENGTH = 10000
    assert_eq!(tl.max_command_length, 10_000, "max_command_length");
    // organon::builtins::communication::MESSAGE_MAX_LEN = 4000
    assert_eq!(tl.message_max_len, 4_000, "message_max_len");
    // organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN = 100000
    assert_eq!(tl.inter_session_max_message_len, 100_000, "inter_session_max_message_len");
    // organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS = 300s
    assert_eq!(tl.inter_session_max_timeout_secs, 300, "inter_session_max_timeout_secs");

    let mc = MessagingConfig::default();
    // agora::semeion::DEFAULT_POLL_INTERVAL = 2s (2000ms)
    assert_eq!(mc.poll_interval_ms, 2_000, "poll_interval_ms");
    // agora::semeion::DEFAULT_BUFFER_CAPACITY = 100
    assert_eq!(mc.buffer_capacity, 100, "buffer_capacity");
    // agora::semeion::CIRCUIT_BREAKER_THRESHOLD = 5
    assert_eq!(mc.circuit_breaker_threshold, 5, "circuit_breaker_threshold");
    // agora::semeion::HALTED_HEALTH_CHECK_INTERVAL = 60s
    assert_eq!(mc.halted_health_check_interval_secs, 60, "halted_health_check_interval_secs");
    // agora::semeion::client::RPC_TIMEOUT = 10s
    assert_eq!(mc.rpc_timeout_secs, 10, "rpc_timeout_secs");
    // agora::semeion::client::HEALTH_TIMEOUT = 2s
    assert_eq!(mc.health_timeout_secs, 2, "health_timeout_secs");
    // agora::semeion::client::RECEIVE_TIMEOUT = 15s
    assert_eq!(mc.receive_timeout_secs, 15, "receive_timeout_secs");
    // organon::builtins::agent::DEFAULT_TIMEOUT_SECS = 300s
    assert_eq!(mc.agent_dispatch_timeout_secs, 300, "agent_dispatch_timeout_secs");
}

#[test]
fn per_agent_defaults_match_original_constants() {
    let ab = AgentBehaviorDefaults::default();

    // Safety
    assert_eq!(ab.safety_loop_detection_threshold, 3, "safety_loop_detection_threshold");
    assert_eq!(ab.safety_consecutive_error_threshold, 4, "safety_consecutive_error_threshold");
    assert_eq!(ab.safety_loop_max_warnings, 2, "safety_loop_max_warnings");
    assert_eq!(ab.safety_session_token_cap, 500_000, "safety_session_token_cap");
    assert_eq!(ab.safety_max_consecutive_tool_only_iterations, 3, "safety_max_consecutive_tool_only_iterations");

    // Hooks
    assert!(ab.hooks_cost_control_enabled, "hooks_cost_control_enabled");
    assert_eq!(ab.hooks_turn_token_budget, 0, "hooks_turn_token_budget");
    assert!(ab.hooks_scope_enforcement_enabled, "hooks_scope_enforcement_enabled");
    assert!(ab.hooks_correction_hooks_enabled, "hooks_correction_hooks_enabled");
    assert!(ab.hooks_audit_logging_enabled, "hooks_audit_logging_enabled");

    // Distillation — nous::distillation constants
    assert_eq!(ab.distillation_context_token_trigger, 120_000, "distillation_context_token_trigger");
    assert_eq!(ab.distillation_message_count_trigger, 150, "distillation_message_count_trigger");
    assert_eq!(ab.distillation_stale_session_days, 7, "distillation_stale_session_days");
    assert_eq!(ab.distillation_stale_min_messages, 20, "distillation_stale_min_messages");
    assert_eq!(ab.distillation_never_distilled_trigger, 30, "distillation_never_distilled_trigger");
    assert_eq!(ab.distillation_legacy_min_messages, 10, "distillation_legacy_min_messages");
    // melete::distill::MAX_BACKOFF_TURNS = 8
    assert_eq!(ab.distillation_max_backoff_turns, 8, "distillation_max_backoff_turns");

    // Competence — nous::competence constants
    assert!((ab.competence_correction_penalty - 0.05).abs() < f64::EPSILON, "competence_correction_penalty");
    assert!((ab.competence_success_bonus - 0.02).abs() < f64::EPSILON, "competence_success_bonus");
    assert!((ab.competence_disagreement_penalty - 0.01).abs() < f64::EPSILON, "competence_disagreement_penalty");
    assert!((ab.competence_min_score - 0.1).abs() < f64::EPSILON, "competence_min_score");
    assert!((ab.competence_max_score - 0.95).abs() < f64::EPSILON, "competence_max_score");
    assert!((ab.competence_default_score - 0.5).abs() < f64::EPSILON, "competence_default_score");
    assert!((ab.competence_escalation_failure_threshold - 0.30).abs() < f64::EPSILON, "competence_escalation_failure_threshold");
    assert_eq!(ab.competence_escalation_min_samples, 5, "competence_escalation_min_samples");

    // Drift — nous::drift constants
    assert_eq!(ab.drift_window_size, 20, "drift_window_size");
    assert_eq!(ab.drift_recent_size, 5, "drift_recent_size");
    assert!((ab.drift_deviation_threshold - 2.0).abs() < f64::EPSILON, "drift_deviation_threshold");
    assert_eq!(ab.drift_min_samples, 8, "drift_min_samples");

    // Uncertainty
    assert_eq!(ab.uncertainty_max_calibration_points, 1_000, "uncertainty_max_calibration_points");

    // Skills
    assert_eq!(ab.skills_max_skills, 5, "skills_max_skills");
    // nous::skills::MAX_CONTEXT_CHARS = 200
    assert_eq!(ab.skills_max_context_chars, 200, "skills_max_context_chars");

    // Working state
    assert_eq!(ab.working_state_ttl_secs, 604_800, "working_state_ttl_secs");

    // Planning — dianoia::stuck constants
    // dianoia::plan::DEFAULT_MAX_ITERATIONS = 10
    assert_eq!(ab.planning_max_iterations, 10, "planning_max_iterations");
    assert_eq!(ab.planning_stuck_history_window, 20, "planning_stuck_history_window");
    assert_eq!(ab.planning_stuck_repeated_error_threshold, 3, "planning_stuck_repeated_error_threshold");
    assert_eq!(ab.planning_stuck_same_args_threshold, 3, "planning_stuck_same_args_threshold");
    assert_eq!(ab.planning_stuck_alternating_threshold, 3, "planning_stuck_alternating_threshold");
    assert_eq!(ab.planning_stuck_escalating_retry_threshold, 3, "planning_stuck_escalating_retry_threshold");

    // Knowledge tuning — episteme constants
    assert_eq!(ab.knowledge_instinct_min_observations, 5, "knowledge_instinct_min_observations");
    assert!((ab.knowledge_instinct_min_success_rate - 0.80).abs() < f64::EPSILON, "knowledge_instinct_min_success_rate");
    assert!((ab.knowledge_instinct_stability_hours - 168.0).abs() < f64::EPSILON, "knowledge_instinct_stability_hours");
    // episteme::surprise::DEFAULT_THRESHOLD = 2.0
    assert!((ab.knowledge_surprise_threshold - 2.0).abs() < f64::EPSILON, "knowledge_surprise_threshold");
    // episteme::surprise::EMA_ALPHA = 0.3
    assert!((ab.knowledge_surprise_ema_alpha - 0.3).abs() < f64::EPSILON, "knowledge_surprise_ema_alpha");
    // episteme::rule_proposals::MIN_OBSERVATIONS = 5
    assert_eq!(ab.knowledge_rule_min_observations, 5, "knowledge_rule_min_observations");
    // episteme::rule_proposals::MIN_CONFIDENCE = 0.60
    assert!((ab.knowledge_rule_min_confidence - 0.60).abs() < f64::EPSILON, "knowledge_rule_min_confidence");
    // episteme::dedup::WEIGHT_NAME = 0.4
    assert!((ab.knowledge_dedup_weight_name - 0.4).abs() < f64::EPSILON, "knowledge_dedup_weight_name");
    // episteme::dedup::WEIGHT_EMBED = 0.3
    assert!((ab.knowledge_dedup_weight_embed - 0.3).abs() < f64::EPSILON, "knowledge_dedup_weight_embed");
    // episteme::dedup::WEIGHT_TYPE = 0.2
    assert!((ab.knowledge_dedup_weight_type - 0.2).abs() < f64::EPSILON, "knowledge_dedup_weight_type");
    // episteme::dedup::WEIGHT_ALIAS = 0.1
    assert!((ab.knowledge_dedup_weight_alias - 0.1).abs() < f64::EPSILON, "knowledge_dedup_weight_alias");
    // episteme::dedup::JW_THRESHOLD = 0.85
    assert!((ab.knowledge_dedup_jw_threshold - 0.85).abs() < f64::EPSILON, "knowledge_dedup_jw_threshold");
    // episteme::dedup::EMBED_THRESHOLD = 0.80
    assert!((ab.knowledge_dedup_embed_threshold - 0.80).abs() < f64::EPSILON, "knowledge_dedup_embed_threshold");

    // Fact lifecycle — eidos::knowledge::fact constants
    assert!((ab.fact_active_threshold - 0.7).abs() < f64::EPSILON, "fact_active_threshold");
    assert!((ab.fact_fading_threshold - 0.3).abs() < f64::EPSILON, "fact_fading_threshold");
    assert!((ab.fact_dormant_threshold - 0.1).abs() < f64::EPSILON, "fact_dormant_threshold");

    // Similarity
    assert!((ab.similarity_threshold - 0.85).abs() < f64::EPSILON, "similarity_threshold");

    // Tool behavior — organon constants
    // organon::builtins::agent::MAX_DISPATCH_TASKS = 10
    assert_eq!(ab.tool_agent_dispatch_max_tasks, 10, "tool_agent_dispatch_max_tasks");
    // organon::builtins::memory::datalog::DEFAULT_ROW_LIMIT = 100
    assert_eq!(ab.tool_datalog_default_row_limit, 100, "tool_datalog_default_row_limit");
    // organon::builtins::memory::datalog::DEFAULT_TIMEOUT_SECS = 5.0
    assert!((ab.tool_datalog_default_timeout_secs - 5.0).abs() < f64::EPSILON, "tool_datalog_default_timeout_secs");
    // organon::builtins::view_file::MAX_IMAGE_BYTES = 20 MiB
    assert_eq!(ab.tool_max_image_bytes, 20_971_520, "tool_max_image_bytes");
    // organon::builtins::view_file::MAX_PDF_BYTES = 32 MiB
    assert_eq!(ab.tool_max_pdf_bytes, 33_554_432, "tool_max_pdf_bytes");

    // Corrections
    // nous::hooks::builtins::correction::MAX_CORRECTIONS = 50
    assert_eq!(ab.corrections_max_corrections, 50, "corrections_max_corrections");
}

#[test]
fn default_config_validates() {
    let config = AletheiaConfig::default();
    assert!(
        crate::validate::validate_config(&config).is_ok(),
        "default config must validate cleanly with new sections"
    );
}

#[test]
fn resolve_nous_uses_defaults_when_no_override() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "test-agent");
    assert_eq!(
        resolved.behavior.distillation_context_token_trigger, 120_000,
        "default behavior should be used when no per-agent override is set"
    );
    assert!((resolved.behavior.competence_correction_penalty - 0.05).abs() < f64::EPSILON,
        "default competence_correction_penalty must come from AgentBehaviorDefaults");
}

#[test]
fn resolve_nous_per_agent_override_wins() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "custom".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/custom".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: Some(AgentBehaviorDefaults {
            competence_correction_penalty: 0.10,
            ..Default::default()
        }),
    });
    let resolved = resolve_nous(&config, "custom");
    assert!(
        (resolved.behavior.competence_correction_penalty - 0.10).abs() < f64::EPSILON,
        "per-agent behavior override must win over defaults"
    );
    // All other fields should remain at default
    assert_eq!(
        resolved.behavior.distillation_context_token_trigger, 120_000,
        "non-overridden fields must retain default values"
    );
}

#[test]
fn resolve_nous_non_overriding_agent_uses_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "plain".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/plain".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
        behavior: None,
    });
    let resolved = resolve_nous(&config, "plain");
    assert_eq!(
        resolved.behavior.corrections_max_corrections, 50,
        "agent without behavior override should use shared defaults"
    );
}

#[test]
fn new_deployment_sections_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.nous_behavior.degraded_panic_threshold = 10;
    config.knowledge.conflict_max_candidates = 8;
    config.provider_behavior.complexity_low_threshold = 25;
    config.api_limits.max_history_limit = 500;
    config.daemon_behavior.prosoche_anomaly_sample_size = 20;
    config.tool_limits.subprocess_timeout_secs = 120;
    config.messaging.circuit_breaker_threshold = 3;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.nous_behavior.degraded_panic_threshold, 10, "nous_behavior survives roundtrip");
    assert_eq!(back.knowledge.conflict_max_candidates, 8, "knowledge survives roundtrip");
    assert_eq!(back.provider_behavior.complexity_low_threshold, 25, "provider_behavior survives roundtrip");
    assert_eq!(back.api_limits.max_history_limit, 500, "api_limits survives roundtrip");
    assert_eq!(back.daemon_behavior.prosoche_anomaly_sample_size, 20, "daemon_behavior survives roundtrip");
    assert_eq!(back.tool_limits.subprocess_timeout_secs, 120, "tool_limits survives roundtrip");
    assert_eq!(back.messaging.circuit_breaker_threshold, 3, "messaging survives roundtrip");
}

#[test]
fn agent_behavior_defaults_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.behavior.distillation_context_token_trigger = 80_000;
    config.agents.defaults.behavior.competence_correction_penalty = 0.08;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.agents.defaults.behavior.distillation_context_token_trigger, 80_000,
        "agent behavior default survives serde roundtrip"
    );
    assert!((back.agents.defaults.behavior.competence_correction_penalty - 0.08).abs() < f64::EPSILON,
        "competence_correction_penalty survives serde roundtrip");
}

#[test]
fn new_deployment_sections_are_absent_in_default_toml() {
    // WHY: operators must be able to omit all new sections from aletheia.toml
    // and still get identical behaviour. This confirms `#[serde(default)]` works.
    let json = r"{}";

    let config: AletheiaConfig = serde_json::from_str(json).expect("parse empty json");
    assert_eq!(
        config.nous_behavior.degraded_panic_threshold, 5,
        "omitted nousBehavior section should use defaults"
    );
    assert_eq!(
        config.api_limits.idempotency_capacity, 10_000,
        "omitted apiLimits section should use defaults"
    );
    assert_eq!(
        config.messaging.poll_interval_ms, 2_000,
        "omitted messaging section should use defaults"
    );
}

mod proptests {
    use proptest::prelude::*;

    use super::*;

    fn arb_channel_binding() -> impl Strategy<Value = ChannelBinding> {
        (
            "[a-z]{3,10}",
            "[a-zA-Z0-9+*]{1,20}",
            "[a-z]{2,8}",
            proptest::option::of("[a-z{}]{1,20}"),
        )
            .prop_map(|(channel, source, nous_id, session_key)| ChannelBinding {
                channel,
                source,
                nous_id,
                session_key: session_key.unwrap_or_else(default_session_pattern),
            })
    }

    proptest! {
        #[test]
        fn channel_binding_roundtrip(binding in arb_channel_binding()) {
            let json = serde_json::to_string(&binding).expect("serialize");
            let back: ChannelBinding = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(&binding.channel, &back.channel);
            prop_assert_eq!(&binding.source, &back.source);
            prop_assert_eq!(&binding.nous_id, &back.nous_id);
            prop_assert_eq!(&binding.session_key, &back.session_key);
        }
    }
}
