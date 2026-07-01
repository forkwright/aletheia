// kanon:ignore RUST/file-too-long — comprehensive validation tests for all config sections
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec[0] is valid after asserting errors are non-empty"
)]

use serde_json::json;

use super::*;

#[test]
fn rejects_stale_timeout_seconds_field() {
    // WHY: `agents.defaults.timeoutSeconds` was documented and scaffolded but
    // never existed on `AgentDefaults`; it is now rejected at parse time so
    // operators get a loud error instead of a silent no-op. (#5788)
    let json = r#"{"agents": {"defaults": {"timeoutSeconds": 300}}}"#;
    let result: Result<crate::config::AletheiaConfig, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "stale timeoutSeconds field should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("timeoutSeconds") || err.contains("unknown field"),
        "error should mention the unknown field: {err}"
    );
}

#[test]
fn rejects_excessive_tool_iterations() {
    let section = json!({ "defaults": { "maxToolIterations": 10_001 } });
    let result = validate_section("agents", &section);
    assert!(
        result.is_err(),
        "maxToolIterations exceeding 10000 should be rejected"
    );
}

#[test]
fn accepts_tool_iterations_within_unrestricted_range() {
    let section = json!({ "defaults": { "maxToolIterations": 10_000 } });
    assert!(
        validate_section("agents", &section).is_ok(),
        "maxToolIterations at 10000 should be accepted"
    );
}

#[test]
fn rejects_invalid_port() {
    let section = json!({ "port": 0 });
    let result = validate_section("gateway", &section);
    assert!(result.is_err(), "port 0 should be rejected");

    let section = json!({ "port": 70000 });
    let result = validate_section("gateway", &section);
    assert!(
        result.is_err(),
        "port 70000 exceeding 65535 should be rejected"
    );
}

#[test]
fn rejects_warn_exceeding_alert() {
    let section = json!({
        "dbMonitoring": { "warnThresholdMb": 500, "alertThresholdMb": 100 }
    });
    let result = validate_section("maintenance", &section);
    assert!(
        result.is_err(),
        "warn threshold exceeding alert threshold should be rejected"
    );
}

#[test]
fn accepts_valid_agents() {
    let section = json!({
        "defaults": {
            "contextTokens": 200_000,
            "maxToolIterations": 200,
            "thinkingBudget": 10_000
        }
    });
    assert!(
        validate_section("agents", &section).is_ok(),
        "valid agent defaults should be accepted"
    );
}

#[test]
fn accepts_valid_gateway() {
    let section = json!({ "port": 8080, "cors": { "maxAgeSecs": 3600 } });
    assert!(
        validate_section("gateway", &section).is_ok(),
        "valid gateway config should be accepted"
    );
}

#[test]
fn accepts_rate_limit_with_trust_proxy_when_enabled() {
    let section = json!({
        "rateLimit": {
            "enabled": true,
            "requestsPerMinute": 60,
            "trustProxy": true
        }
    });
    assert!(
        validate_section("gateway", &section).is_ok(),
        "trustProxy may be true when rate limiting is enabled"
    );
}

#[test]
fn rejects_trust_proxy_without_enabled_rate_limit() {
    let section = json!({
        "rateLimit": {
            "enabled": false,
            "trustProxy": true
        }
    });
    let result = validate_section("gateway", &section);
    assert!(
        result.is_err(),
        "trustProxy without enabled rate limiting is a dead config branch"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("trustProxy")),
        "error should mention trustProxy: {err:?}"
    );
}

#[test]
fn rejects_enabled_rate_limit_with_zero_requests_per_minute() {
    let section = json!({
        "rateLimit": {
            "enabled": true,
            "requestsPerMinute": 0
        }
    });
    let result = validate_section("gateway", &section);
    assert!(
        result.is_err(),
        "enabled rate limiting must have a positive requestsPerMinute"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("requestsPerMinute")),
        "error should mention requestsPerMinute: {err:?}"
    );
}

#[test]
fn rejects_disabled_csrf_without_acknowledgement() {
    let section = json!({ "csrf": { "enabled": false } });
    let result = validate_section("gateway", &section);
    assert!(
        result.is_err(),
        "disabled csrf must require explicit acknowledgement"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("disableAcknowledged")),
        "error should mention the acknowledgement field: {err:?}"
    );
}

#[test]
fn accepts_disabled_csrf_with_acknowledgement() {
    let section = json!({ "csrf": { "enabled": false, "disableAcknowledged": true } });
    assert!(
        validate_section("gateway", &section).is_ok(),
        "disabled csrf should be accepted only with explicit acknowledgement"
    );
}

#[test]
fn unknown_section_errors() {
    let result = validate_section("nonexistent", &json!({}));
    assert!(result.is_err(), "unknown config section should be rejected");
}

#[test]
fn rejects_bootstrap_exceeding_context() {
    let section = json!({
        "defaults": {
            "contextTokens": 10_000,
            "bootstrapMaxTokens": 20_000
        }
    });
    let result = validate_section("agents", &section);
    assert!(
        result.is_err(),
        "bootstrapMaxTokens exceeding contextTokens should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors[0].contains("bootstrapMaxTokens"),
        "error should mention bootstrapMaxTokens"
    );
}

#[test]
fn accepts_bootstrap_within_context() {
    let section = json!({
        "defaults": {
            "contextTokens": 200_000,
            "bootstrapMaxTokens": 40_000
        }
    });
    assert!(
        validate_section("agents", &section).is_ok(),
        "bootstrapMaxTokens within contextTokens should be accepted"
    );
}

#[test]
fn rejects_invalid_auth_mode() {
    let section = json!({ "auth": { "mode": "magic" } });
    let result = validate_section("gateway", &section);
    assert!(result.is_err(), "invalid auth mode should be rejected");
    let err = result.unwrap_err();
    assert!(err.errors[0].contains("gateway.auth.mode"));
}

#[test]
fn accepts_valid_auth_modes() {
    for mode in &["token", "jwt"] {
        let section = json!({ "auth": { "mode": mode } });
        assert!(
            validate_section("gateway", &section).is_ok(),
            "mode '{mode}' should be valid"
        );
    }
}

#[test]
fn accepts_provider_type_and_deployment_aliases() {
    let section = json!([
        {
            "name": "openai-cloud",
            "providerType": "openai",
            "deploymentTarget": "cloud",
            "models": ["gpt-4.1"]
        },
        {
            "name": "local-chat",
            "providerType": "open-ai-compatible",
            "baseUrl": "http://127.0.0.1:5001/v1",
            "deploymentTarget": "local_hosted",
            "models": ["anubis-70b"]
        },
        {
            "name": "local-vlm",
            "providerType": "openai-compatible",
            "baseUrl": "http://127.0.0.1:5009/v1",
            "deploymentTarget": "localhosted",
            "models": ["qwen3-vl"]
        },
        {
            "name": "codex-seat",
            "providerType": "codex_oauth",
            "binary": "bin/codex",
            "workdir": "workspace",
            "timeoutSecs": 30,
            "deploymentTarget": "cloud",
            "models": ["codex/gpt-5-codex"]
        }
    ]);

    assert!(
        validate_section("providers", &section).is_ok(),
        "provider/deployment aliases should validate"
    );
}

#[test]
fn provider_aliases_deserialize_to_typed_config() {
    let json = r#"{
        "providers": [
            {
                "name": "openai-cloud",
                "providerType": "openai",
                "deploymentTarget": "cloud",
                "models": ["gpt-4.1"]
            },
            {
                "name": "local-chat",
                "providerType": "openai-compatible",
                "baseUrl": "http://127.0.0.1:5001/v1",
                "deploymentTarget": "local_hosted",
                "models": ["anubis-70b"]
            },
            {
                "name": "codex-seat",
                "providerType": "codex-oauth",
                "binary": "bin/codex",
                "workdir": "workspace",
                "timeoutSecs": 30,
                "deploymentTarget": "cloud",
                "models": ["codex/gpt-5-codex"]
            }
        ]
    }"#;

    let config_result: Result<crate::config::AletheiaConfig, _> = serde_json::from_str(json);
    assert!(
        config_result.is_ok(),
        "provider aliases should parse: {config_result:?}"
    );
    let config = config_result.unwrap_or_default();
    assert_eq!(config.providers.len(), 3);
    assert_eq!(
        config.providers[0].kind,
        crate::config::ProviderKind::OpenAi
    );
    assert_eq!(
        config.providers[1].kind,
        crate::config::ProviderKind::OpenAiCompatible
    );
    assert_eq!(
        config.providers[1].deployment_target,
        crate::config::DeploymentTarget::LocalHosted
    );
    assert_eq!(
        config.providers[2].kind,
        crate::config::ProviderKind::CodexOauth
    );
    assert_eq!(
        config.providers[2].deployment_target,
        crate::config::DeploymentTarget::Cloud
    );
    assert_eq!(
        config.providers[2].binary.as_deref(),
        Some(std::path::Path::new("bin/codex"))
    );
    assert_eq!(
        config.providers[2].workdir.as_deref(),
        Some(std::path::Path::new("workspace"))
    );
    assert_eq!(config.providers[2].timeout_secs, Some(30));
}

#[test]
fn rejects_invalid_subprocess_provider_fields() {
    let section = json!([
        {
            "name": "cc-seat",
            "providerType": "claude-code",
            "baseUrl": "https://api.anthropic.com",
            "timeoutSecs": 1,
            "models": [""]
        },
        {
            "name": "openai-cloud",
            "providerType": "openai",
            "binary": "bin/openai"
        }
    ]);

    let result = validate_section("providers", &section);
    assert!(
        result.is_err(),
        "invalid subprocess fields should be rejected"
    );
    let errors = result.unwrap_err().errors.join("\n");
    assert!(
        errors.contains("baseUrl is not valid for subprocess"),
        "HTTP-only fields should be rejected for subprocess providers: {errors}"
    );
    assert!(
        errors.contains("timeoutSecs must be between 5 and 3600"),
        "subprocess timeout range should be enforced: {errors}"
    );
    assert!(
        errors.contains("models[0] must be a non-empty string"),
        "empty model IDs should be rejected: {errors}"
    );
    assert!(
        errors.contains("binary is only valid"),
        "subprocess-only fields should be rejected on HTTP providers: {errors}"
    );
}

/// Serialises tests that mutate `ALETHEIA_ALLOW_AUTH_NONE`. Cargo runs tests
/// within a binary in parallel threads, and `std::env` is process-wide, so
/// without a mutex the opt-in gate flips under another test's feet.
static AUTH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `auth.mode = "none"` via the config API is rejected unless the operator
/// has explicitly opted in via `ALETHEIA_ALLOW_AUTH_NONE=1`. This is the
/// `validate_auth_mode_policy` gate that callers must invoke in addition to
/// the structural `validate_section`. (#3383, #4240)
#[test]
fn auth_mode_none_policy_env_gate() {
    let _guard = AUTH_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    #[expect(
        unsafe_code,
        reason = "std::env::{set_var,remove_var} require unsafe in edition 2024; serialised via AUTH_ENV_LOCK"
    )]
    // SAFETY: `AUTH_ENV_LOCK` serialises all tests in this module that mutate
    // ALLOW_AUTH_NONE_ENV; no other test in the crate touches this var.
    unsafe {
        std::env::remove_var(crate::validate::ALLOW_AUTH_NONE_ENV);
    }

    let section = json!({ "auth": { "mode": "none" } });

    // Without opt-in: policy gate rejects, error points the operator at the env var.
    let rejected = crate::validate::validate_auth_mode_policy(&section);
    assert!(
        rejected.is_err(),
        "auth_mode = none must be rejected by the policy gate without env opt-in"
    );
    let err = rejected.unwrap_err();
    assert!(
        err.errors
            .iter()
            .any(|e| e.contains(crate::validate::ALLOW_AUTH_NONE_ENV)),
        "error should mention the env-var opt-in: {err:?}"
    );

    // WARNING: structural validation alone is permissive — a config PUT must
    // call BOTH validate_section AND validate_auth_mode_policy to preserve
    // the gate.
    let structural = validate_section("gateway", &section);
    assert!(
        structural.is_ok(),
        "validate_section is now structural-only and must accept mode=none: {structural:?}"
    );

    // With opt-in: policy gate accepts.
    #[expect(
        unsafe_code,
        reason = "std::env::set_var requires unsafe in edition 2024; serialised via AUTH_ENV_LOCK"
    )]
    // SAFETY: `AUTH_ENV_LOCK` held for the duration of this test.
    unsafe {
        std::env::set_var(crate::validate::ALLOW_AUTH_NONE_ENV, "1");
    }
    let accepted = crate::validate::validate_auth_mode_policy(&section);

    #[expect(
        unsafe_code,
        reason = "std::env::remove_var requires unsafe in edition 2024; serialised via AUTH_ENV_LOCK"
    )]
    // SAFETY: Cleanup so later tests see an unset var.
    unsafe {
        std::env::remove_var(crate::validate::ALLOW_AUTH_NONE_ENV);
    }

    assert!(
        accepted.is_ok(),
        "auth_mode = none must be accepted with env opt-in: {accepted:?}"
    );
}

/// File-load path (server startup, `check-config`) accepts `auth.mode = "none"`
/// regardless of the env opt-in; the gate is policy-level, not structural.
/// Operators with filesystem control of aletheia.toml are trusted; visibility
/// is preserved via the loud `warn_if_auth_disabled` emission. (#4240)
#[test]
fn validate_section_gateway_accepts_mode_none_without_opt_in() {
    let _guard = AUTH_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    #[expect(
        unsafe_code,
        reason = "std::env::remove_var requires unsafe in edition 2024; serialised via AUTH_ENV_LOCK"
    )]
    // SAFETY: `AUTH_ENV_LOCK` serialises ALLOW_AUTH_NONE_ENV mutations across tests.
    unsafe {
        std::env::remove_var(crate::validate::ALLOW_AUTH_NONE_ENV);
    }

    let section = json!({ "auth": { "mode": "none" } });
    let outcome = validate_section("gateway", &section);
    assert!(
        outcome.is_ok(),
        "validate_section('gateway', mode=none) must accept on the file-load path: {outcome:?}"
    );
}

/// Structural mode validation still rejects unknown auth modes — this is not
/// a policy gate and applies to all paths. (#3383)
#[test]
fn validate_section_gateway_rejects_unknown_auth_mode() {
    let section = json!({ "auth": { "mode": "telepathy" } });
    let result = validate_section("gateway", &section);
    assert!(
        result.is_err(),
        "unknown auth.mode must be rejected as a structural error"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("telepathy")),
        "error should mention the bad mode: {err:?}"
    );
}

#[test]
fn rejects_zero_embedding_dimension() {
    let section = json!({ "dimension": 0 });
    let result = validate_section("embedding", &section);
    assert!(
        result.is_err(),
        "zero embedding dimension should be rejected"
    );
}

#[test]
fn rejects_empty_embedding_provider() {
    let section = json!({ "provider": "" });
    let result = validate_section("embedding", &section);
    assert!(
        result.is_err(),
        "empty embedding provider should be rejected"
    );
}

#[test]
fn accepts_valid_embedding() {
    let section = json!({ "provider": "candle", "dimension": 384 });
    assert!(
        validate_section("embedding", &section).is_ok(),
        "valid embedding config should be accepted"
    );
}

#[test]
fn rejects_unknown_embedding_provider() {
    let section = json!({ "provider": "telepathy", "dimension": 384 });
    let result = validate_section("embedding", &section);
    assert!(
        result.is_err(),
        "unknown embedding provider should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|error| error.contains("telepathy")),
        "error should mention the unknown provider: {err:?}"
    );
}

#[test]
fn rejects_openai_compat_without_base_url() {
    let section = json!({ "provider": "openai-compat", "dimension": 384 });
    let result = validate_section("embedding", &section);
    assert!(
        result.is_err(),
        "openai-compatible embedding config should require an endpoint"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|error| error.contains("baseUrl")),
        "error should mention embedding.baseUrl: {err:?}"
    );
}

#[test]
fn rejects_empty_embedding_api_key_env() {
    let section = json!({
        "provider": "voyage",
        "apiKeyEnv": " ",
        "dimension": 1024
    });
    let result = validate_section("embedding", &section);
    assert!(
        result.is_err(),
        "empty embedding apiKeyEnv should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|error| error.contains("apiKeyEnv")),
        "error should mention embedding.apiKeyEnv: {err:?}"
    );
}

#[test]
fn accepts_openai_compat_with_base_url() {
    let section = json!({
        "provider": "openai-compat",
        "baseUrl": "http://127.0.0.1:5005/v1",
        "dimension": 384
    });
    assert!(
        validate_section("embedding", &section).is_ok(),
        "openai-compatible embedding config with endpoint should be accepted structurally"
    );
}

#[test]
fn accepts_valid_external_http_tool() {
    let section = json!({
        "requiredFailureMode": "fail_startup",
        "required": {
            "search": {
                "type": "http",
                "endpoint": "http://localhost:3100",
                "method": "post",
                "groups": ["mcp"],
                "reversibility": "irreversible",
                "description": "Search service"
            }
        },
        "optional": {
            "reader": {
                "type": "http",
                "endpoint": "https://example.com/api",
                "method": "get"
            }
        }
    });
    assert!(
        validate_section("tools", &section).is_ok(),
        "valid external tools should be accepted"
    );
}

#[test]
fn accepts_explicit_degraded_required_tool_failure_mode() {
    let section = json!({
        "requiredFailureMode": "degraded",
        "required": {
            "search": {
                "type": "http",
                "endpoint": "http://localhost:3100",
                "method": "post",
                "groups": ["mcp"],
                "reversibility": "irreversible"
            }
        }
    });
    assert!(
        validate_section("tools", &section).is_ok(),
        "operators should be able to opt into degraded startup for required tool failures"
    );
}

#[test]
fn rejects_invalid_required_tool_failure_mode() {
    let section = json!({
        "requiredFailureMode": "warn",
        "required": {}
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "unknown required tool failure mode should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("requiredFailureMode") && err.contains("fail_startup"),
        "error should identify valid failure modes: {err}"
    );
}

#[test]
fn rejects_required_http_tool_missing_safety_policy() {
    let section = json!({
        "required": {
            "bad": {
                "type": "http",
                "endpoint": "http://localhost:3100",
                "method": "post"
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "required HTTP tool without policy should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("groups") && err.contains("reversibility"),
        "error should mention both missing policy fields: {err}"
    );
}

#[test]
fn accepts_optional_http_tool_missing_safety_policy() {
    let section = json!({
        "optional": {
            "reader": {
                "type": "http",
                "endpoint": "https://example.com/api",
                "method": "get"
            }
        }
    });
    assert!(
        validate_section("tools", &section).is_ok(),
        "optional HTTP tools may rely on conservative runtime defaults"
    );
}

#[test]
fn rejects_http_tool_invalid_safety_policy() {
    let section = json!({
        "required": {
            "bad": {
                "type": "http",
                "endpoint": "http://localhost:3100",
                "groups": ["unknown"],
                "reversibility": "instant_undo"
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "invalid HTTP tool policy should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("invalid group") && err.contains("instant_undo"),
        "error should identify invalid policy values: {err}"
    );
}

#[test]
fn rejects_http_tool_missing_endpoint() {
    let section = json!({
        "required": {
            "bad": {
                "type": "http",
                "groups": ["mcp"],
                "reversibility": "irreversible"
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "http tool without endpoint should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("endpoint")),
        "error should mention endpoint: {err:?}"
    );
}

#[test]
fn rejects_tool_missing_type() {
    let section = json!({
        "required": {
            "bad": { "endpoint": "http://localhost:3100" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "tool without type should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("type is required")),
        "error should mention missing type: {err:?}"
    );
}

#[test]
fn rejects_http_tool_non_http_url() {
    let section = json!({
        "required": {
            "bad": { "type": "http", "endpoint": "ftp://example.com" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "non-HTTP endpoint scheme should be rejected"
    );
}

#[test]
fn rejects_mcp_tool_without_endpoint_or_command() {
    let section = json!({
        "required": {
            "bad": { "type": "mcp" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "mcp tool without endpoint or command should be rejected"
    );
}

#[test]
fn rejects_invalid_tool_kind() {
    let section = json!({
        "required": {
            "bad": { "type": "websocket" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "invalid tool kind should be rejected");
}

#[test]
fn rejects_invalid_http_method() {
    let section = json!({
        "required": {
            "bad": { "type": "http", "endpoint": "http://localhost", "method": "trace" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "invalid http method should be rejected");
}

#[test]
fn accepts_mcp_tool_with_valid_bearer_auth() {
    let section = json!({
        "required": {
            "secure": {
                "type": "mcp",
                "endpoint": "https://mcp.example.com",
                "auth": { "type": "bearer", "token": "secret-token" }
            }
        }
    });
    assert!(
        validate_section("tools", &section).is_ok(),
        "valid bearer auth should be accepted"
    );
}

#[test]
fn rejects_mcp_tool_with_empty_bearer_token() {
    let section = json!({
        "required": {
            "secure": {
                "type": "mcp",
                "endpoint": "https://mcp.example.com",
                "auth": { "type": "bearer", "token": "" }
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "empty bearer token should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("token"), "error should mention token: {err}");
}

#[test]
fn rejects_mcp_tool_with_invalid_auth_type() {
    let section = json!({
        "required": {
            "secure": {
                "type": "mcp",
                "endpoint": "https://mcp.example.com",
                "auth": { "type": "hmac" }
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "invalid auth type should be rejected");
}

#[test]
fn rejects_mcp_tool_with_missing_env_token_field() {
    let section = json!({
        "required": {
            "secure": {
                "type": "mcp",
                "endpoint": "https://mcp.example.com",
                "auth": { "type": "env_token", "header_name": "X-Api-Key" }
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "env_token without env_var should be rejected"
    );
}

#[test]
fn rejects_mcp_tool_with_missing_custom_header_value() {
    let section = json!({
        "required": {
            "secure": {
                "type": "mcp",
                "endpoint": "https://mcp.example.com",
                "auth": { "type": "header", "name": "X-Api-Key" }
            }
        }
    });
    let result = validate_section("tools", &section);
    assert!(
        result.is_err(),
        "header auth without value should be rejected"
    );
}

#[test]
fn rejects_invalid_tool_name() {
    let section = json!({
        "required": {
            "bad name!": { "type": "builtin" }
        }
    });
    let result = validate_section("tools", &section);
    assert!(result.is_err(), "invalid tool name should be rejected");
}

#[test]
fn rejects_invalid_signal_port() {
    let section = json!({
        "signal": {
            "accounts": {
                "primary": { "httpPort": 0 }
            }
        }
    });
    let result = validate_section("channels", &section);
    assert!(result.is_err(), "zero signal httpPort should be rejected");
}

#[test]
fn accepts_valid_channels() {
    let section = json!({
        "signal": {
            "accounts": {
                "primary": { "httpPort": 8080 }
            }
        },
        "matrix": {
            "accounts": {
                "primary": {
                    "homeserver": "https://matrix.example.org",
                    "accessTokenEnv": "MATRIX_ACCESS_TOKEN"
                }
            }
        }
    });
    assert!(
        validate_section("channels", &section).is_ok(),
        "valid channel config should be accepted"
    );
}

#[test]
fn rejects_matrix_account_missing_token_env() {
    let section = json!({
        "matrix": {
            "accounts": {
                "primary": {
                    "homeserver": "https://matrix.example.org",
                    "accessTokenEnv": ""
                }
            }
        }
    });
    let result = validate_section("channels", &section);
    assert!(
        result.is_err(),
        "matrix account without accessTokenEnv should be rejected"
    );
}

#[test]
fn rejects_binding_with_empty_fields() {
    let section = json!([
        { "channel": "signal", "source": "*", "nousId": "" }
    ]);
    let result = validate_section("bindings", &section);
    assert!(
        result.is_err(),
        "binding with empty nousId should be rejected"
    );
}

#[test]
fn accepts_valid_bindings() {
    let section = json!([
        { "channel": "signal", "source": "*", "nousId": "main" }
    ]);
    assert!(
        validate_section("bindings", &section).is_ok(),
        "valid binding config should be accepted"
    );
}

#[test]
fn accepts_matrix_binding() {
    let section = json!([
        { "channel": "matrix", "source": "!room:example.org", "nousId": "main" }
    ]);
    assert!(
        validate_section("bindings", &section).is_ok(),
        "matrix binding config should be accepted"
    );
}

#[test]
fn accepts_feature_flags() {
    let section = json!([
        { "key": "new_ui", "description": "Enable the new UI", "enabled": true }
    ]);
    assert!(
        validate_section("feature_flags", &section).is_ok(),
        "feature flag config should be accepted"
    );
}

#[test]
fn rejects_invalid_feature_flags() {
    let section = json!([
        { "key": "", "description": "" },
        { "key": "new_ui", "description": "Enable the new UI" },
        { "key": "new_ui", "description": "Duplicate key" }
    ]);
    let result = validate_section("feature_flags", &section);
    assert!(result.is_err(), "invalid feature flags should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.errors
            .iter()
            .any(|e| e.contains("feature_flags[0].key")),
        "empty feature flag key should be reported"
    );
    assert!(
        err.errors
            .iter()
            .any(|e| e.contains("feature_flags[0].description")),
        "empty feature flag description should be reported"
    );
    assert!(
        err.errors.iter().any(|e| e.contains("duplicated")),
        "duplicate feature flag keys should be reported"
    );
}

#[test]
fn rejects_invalid_credential_source() {
    let section = json!({ "source": "magic" });
    let result = validate_section("credential", &section);
    assert!(
        result.is_err(),
        "invalid credential source should be rejected"
    );
    let err = result.unwrap_err();
    assert!(err.errors[0].contains("credential.source"));
}

#[test]
fn accepts_valid_credential_sources() {
    for source in &["auto", "api-key", "claude-code"] {
        let section = json!({ "source": source });
        assert!(
            validate_section("credential", &section).is_ok(),
            "source '{source}' should be valid"
        );
    }
}

#[test]
fn rejects_invalid_agency_level() {
    let section = json!({ "defaults": { "agency": "yolo" } });
    let result = validate_section("agents", &section);
    assert!(result.is_err(), "invalid agency level should be rejected");
    let err = result.unwrap_err();
    assert!(err.errors[0].contains("agency"));
}

#[test]
fn accepts_valid_agency_levels() {
    for level in &["unrestricted", "standard", "restricted"] {
        let section = json!({ "defaults": { "agency": level } });
        assert!(
            validate_section("agents", &section).is_ok(),
            "agency level '{level}' should be valid"
        );
    }
}

#[test]
fn rejects_empty_model_primary() {
    let section = json!({ "defaults": { "model": { "primary": "" } } });
    let result = validate_section("agents", &section);
    assert!(result.is_err(), "empty model primary should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("model.primary")),
        "expected model.primary error, got: {err:?}"
    );
}

#[test]
fn rejects_empty_model_fallback() {
    let section =
        json!({ "defaults": { "model": { "primary": "claude-sonnet-4-6", "fallbacks": [""] } } });
    let result = validate_section("agents", &section);
    assert!(result.is_err(), "empty model fallback should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("fallbacks[0]")),
        "expected fallbacks[0] error, got: {err:?}"
    );
}

#[test]
fn accepts_valid_model_ids() {
    let section = json!({
        "defaults": {
            "model": {
                "primary": "claude-sonnet-4-6",
                "fallbacks": ["claude-haiku-3-5"]
            }
        }
    });
    assert!(
        validate_section("agents", &section).is_ok(),
        "valid model ids should be accepted"
    );
}

#[test]
fn rejects_token_budget_exceeding_maximum() {
    for field in [
        "contextTokens",
        "maxOutputTokens",
        "bootstrapMaxTokens",
        "thinkingBudget",
    ] {
        let section = json!({ "defaults": { field: 1_000_001_u64 } });
        let result = validate_section("agents", &section);
        assert!(
            result.is_err(),
            "{field} exceeding MAX_TOKEN_BUDGET should be rejected"
        );
        let err = result.unwrap_err();
        assert!(
            err.errors.iter().any(|e| e.contains(field)),
            "expected error mentioning {field}, got: {err:?}"
        );
    }
}

#[test]
fn accepts_token_budget_at_maximum() {
    let section = json!({ "defaults": { "contextTokens": 1_000_000_u64 } });
    assert!(
        validate_section("agents", &section).is_ok(),
        "token budget at maximum should be accepted"
    );
}

#[test]
fn validate_config_accepts_defaults() {
    let config = AletheiaConfig::default();
    assert!(
        validate_config(&config).is_ok(),
        "default config should be valid"
    );
}

#[test]
fn validate_config_rejects_invalid_tool_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.max_tool_iterations = 0;

    let result = validate_config(&config);
    assert!(result.is_err(), "zero maxToolIterations should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("maxToolIterations")),
        "error should mention maxToolIterations, got: {err:?}"
    );
}

#[test]
fn validate_config_rejects_required_http_tool_missing_policy() {
    let mut config = AletheiaConfig::default();
    config.tools.required.insert(
        "search".to_owned(),
        crate::config::ExternalToolEntry {
            kind: crate::config::ExternalToolKind::Http,
            endpoint: Some("https://example.com/search".to_owned()),
            command: None,
            args: Vec::new(),
            cwd: None,
            env: std::collections::HashMap::new(),
            description: None,
            method: crate::config::ExternalToolMethod::Post,
            groups: None,
            reversibility: None,
            auth: None,
        },
    );

    let result = validate_config(&config);
    assert!(
        result.is_err(),
        "required HTTP tools without explicit policy should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("tools.required.search.groups")
            && err.contains("tools.required.search.reversibility"),
        "error should identify both missing policy fields: {err}"
    );
}

// ── Behavioral section validators ──

#[test]
fn rejects_invalid_nous_behavior() {
    let section = json!({ "loopDetectionWindow": 1 });
    let result = validate_section("nousBehavior", &section);
    assert!(
        result.is_err(),
        "loopDetectionWindow below minimum should be rejected"
    );
}

#[test]
fn accepts_valid_nous_behavior() {
    let section = json!({
        "loopDetectionWindow": 50,
        "gcIntervalSecs": 300,
        "managerHealthIntervalSecs": 30
    });
    assert!(
        validate_section("nousBehavior", &section).is_ok(),
        "valid nous behavior config should be accepted"
    );
}

#[test]
fn rejects_invalid_knowledge() {
    let section = json!({ "conflictIntraBatchDedupThreshold": 0.1 });
    let result = validate_section("knowledge", &section);
    assert!(
        result.is_err(),
        "intra-batch dedup threshold below 0.5 should be rejected"
    );
}

#[test]
fn rejects_knowledge_min_exceeding_max_fact_length() {
    let section = json!({
        "extractionMinFactLength": 600,
        "extractionMaxFactLength": 500
    });
    let result = validate_section("knowledge", &section);
    assert!(
        result.is_err(),
        "min fact length exceeding max should be rejected"
    );
}

#[test]
fn accepts_valid_knowledge() {
    let section = json!({
        "conflictIntraBatchDedupThreshold": 0.95,
        "conflictMaxCandidates": 5,
        "decayReinforcementBoost": 0.02
    });
    assert!(
        validate_section("knowledge", &section).is_ok(),
        "valid knowledge config should be accepted"
    );
}

#[test]
fn rejects_complexity_low_exceeding_high() {
    let section = json!({
        "complexityLowThreshold": 80,
        "complexityHighThreshold": 30
    });
    let result = validate_section("providerBehavior", &section);
    assert!(
        result.is_err(),
        "low complexity threshold exceeding high should be rejected"
    );
}

#[test]
fn accepts_valid_provider_behavior() {
    let section = json!({
        "nonStreamingTimeoutSecs": 120,
        "complexityLowThreshold": 30,
        "complexityHighThreshold": 70
    });
    assert!(
        validate_section("providerBehavior", &section).is_ok(),
        "valid provider behavior config should be accepted"
    );
}

#[test]
fn rejects_api_limits_default_exceeding_max_history() {
    let section = json!({
        "defaultHistoryLimit": 2000,
        "maxHistoryLimit": 1000
    });
    let result = validate_section("apiLimits", &section);
    assert!(
        result.is_err(),
        "default history limit exceeding max should be rejected"
    );
}

#[test]
fn accepts_valid_api_limits() {
    let section = json!({
        "maxMessageBytes": 262_144,
        "maxHistoryLimit": 1000,
        "defaultHistoryLimit": 50
    });
    assert!(
        validate_section("apiLimits", &section).is_ok(),
        "valid API limits config should be accepted"
    );
}

#[test]
fn rejects_daemon_backoff_base_exceeding_cap() {
    let section = json!({
        "watchdogBackoffBaseSecs": 50,
        "watchdogBackoffCapSecs": 10
    });
    let result = validate_section("daemonBehavior", &section);
    assert!(
        result.is_err(),
        "backoff base exceeding cap should be rejected"
    );
}

#[test]
fn accepts_valid_daemon_behavior() {
    let section = json!({
        "watchdogBackoffBaseSecs": 2,
        "watchdogBackoffCapSecs": 300
    });
    assert!(
        validate_section("daemonBehavior", &section).is_ok(),
        "valid daemon behavior config should be accepted"
    );
}

#[test]
fn rejects_invalid_tool_limits() {
    let section = json!({ "maxPatternLength": 1 });
    let result = validate_section("toolLimits", &section);
    assert!(
        result.is_err(),
        "maxPatternLength below minimum should be rejected"
    );
}

#[test]
fn accepts_valid_tool_limits() {
    let section = json!({
        "maxPatternLength": 1000,
        "subprocessTimeoutSecs": 60,
        "maxWriteBytes": 10_485_760
    });
    assert!(
        validate_section("toolLimits", &section).is_ok(),
        "valid tool limits config should be accepted"
    );
}

#[test]
fn rejects_invalid_messaging() {
    let section = json!({ "pollIntervalMs": 10 });
    let result = validate_section("messaging", &section);
    assert!(
        result.is_err(),
        "pollIntervalMs below 100 should be rejected"
    );
}

#[test]
fn accepts_valid_messaging() {
    let section = json!({
        "pollIntervalMs": 2000,
        "bufferCapacity": 100,
        "circuitBreakerThreshold": 5
    });
    assert!(
        validate_section("messaging", &section).is_ok(),
        "valid messaging config should be accepted"
    );
}

// --- validate_startup instance subdirectory checks (#3338) ---

#[test]
fn validate_startup_rejects_missing_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    // WHY: create data/ and nous/ but not config/ to isolate the check
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("nous")).unwrap();

    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("config")),
        "error should mention missing config/ directory: {err:?}"
    );
}

#[test]
fn validate_startup_rejects_missing_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::create_dir_all(dir.path().join("nous")).unwrap();

    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("data")),
        "error should mention missing data/ directory: {err:?}"
    );
}

#[test]
fn validate_startup_rejects_missing_nous_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();

    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("nous")),
        "error should mention missing nous/ directory: {err:?}"
    );
}

#[test]
fn validate_startup_collects_all_missing_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    // WHY: no subdirectories at all — all three should be reported
    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.len() >= 3,
        "should report at least 3 missing directories, got {}: {err:?}",
        err.errors.len()
    );
}

#[test]
fn validate_startup_passes_with_complete_layout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("nous")).unwrap();

    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    // WHY: an empty agents.list still fails validation, so assert only that
    // no missing-subdirectory error appears — the directory checks are the
    // subject of this test.
    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors
            .iter()
            .all(|e| !e.contains("required instance directory")),
        "no subdirectory errors should be present when layout is complete: {err:?}"
    );
}

#[test]
fn validate_startup_rejects_agent_workspace_missing_soul() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("nous").join("alice")).unwrap();

    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();
    config.agents.list.push(crate::config::NousDefinition {
        id: "alice".to_owned(),
        name: None,
        model: None,
        workspace: "nous/alice".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: false,
        episteme_cohort: None,
        recall: None,
        recall_profile: None,
        behavior: None,
        ..Default::default()
    });

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("SOUL.md")),
        "startup validation should enforce workspace schema: {err:?}"
    );
}

// --- Auth-disabled startup warning (#3383) ---

/// When `gateway.auth.mode = "none"`, `warn_if_auth_disabled` must emit a
/// single `warn!` event prefixed `SECURITY: auth disabled` so operators see
/// the disabled state in every log aggregator.
#[test]
#[tracing_test::traced_test]
// kanon:ignore RUST/doc-promised-observability — test verifies warn! emission from the function under test via tracing_test::traced_test
fn warn_if_auth_disabled_emits_security_warning() {
    let mut config = AletheiaConfig::default();
    config.gateway.auth.mode = "none".to_owned();

    crate::validate::warn_if_auth_disabled(&config);

    assert!(
        logs_contain("SECURITY: auth disabled"),
        "warn should carry the SECURITY prefix so log aggregators surface it"
    );
    assert!(
        logs_contain("WARN"),
        "warning must be at warn level, not info"
    );
}

/// When `gateway.auth.mode != "none"`, no warning is emitted.
#[test]
#[tracing_test::traced_test]
fn warn_if_auth_disabled_silent_when_auth_enabled() {
    let config = AletheiaConfig::default(); // default: mode = "token"
    crate::validate::warn_if_auth_disabled(&config);
    assert!(
        !logs_contain("SECURITY: auth disabled"),
        "no warning should fire when auth is enabled"
    );
}

#[test]
fn validate_startup_error_includes_init_hint() {
    let dir = tempfile::tempdir().unwrap();
    let oikos = crate::oikos::Oikos::from_root(dir.path());
    let mut config = AletheiaConfig::default();
    config.agents.list.clear();

    let err = validate_startup(&config, &oikos).unwrap_err();
    assert!(
        err.errors.iter().any(|e| e.contains("aletheia init")),
        "error should include help hint about `aletheia init`: {err:?}"
    );
}

// WHY(#3716): `is_loopback_bind` is the single point of truth for "is this
// address host-local enough that auth-none is acceptable". The fix wires it
// into server startup and gates a hard refusal on non-loopback + auth.mode =
// none. Pin the classification so future operator-visible addresses don't
// silently slip into the loopback bucket.
#[test]
fn is_loopback_bind_accepts_loopback_forms() {
    assert!(crate::validate::is_loopback_bind("127.0.0.1"));
    assert!(crate::validate::is_loopback_bind("localhost"));
    assert!(crate::validate::is_loopback_bind("::1"));
    assert!(crate::validate::is_loopback_bind("[::1]"));
}

#[test]
fn is_loopback_bind_rejects_wildcard_and_lan() {
    assert!(!crate::validate::is_loopback_bind("0.0.0.0"));
    assert!(!crate::validate::is_loopback_bind("::"));
    // Private RFC1918 and CGNAT fixtures — the digits matter less than the
    // network class; pick addresses that exercise the non-loopback branch
    // without matching the LAN/tailnet PII patterns.
    assert!(!crate::validate::is_loopback_bind("10.0.0.1"));
    assert!(!crate::validate::is_loopback_bind("172.16.0.1"));
    assert!(!crate::validate::is_loopback_bind("host-a.lan"));
}
