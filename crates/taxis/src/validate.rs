//! Config section validation: rejects invalid values before persisting.

use serde_json::Value;
use snafu::Snafu;

use crate::config::AletheiaConfig;

/// Validation error with collected messages.
#[derive(Debug, Snafu)]
#[snafu(display("config validation failed:\n  - {}", errors.join("\n  - ")))]
pub struct ValidationError {
    pub errors: Vec<String>,
    #[snafu(implicit)]
    pub location: snafu::Location,
}

/// Validate an entire [`AletheiaConfig`] by checking each section.
///
/// Serializes to JSON and validates each top-level section using
/// [`validate_section`]. Returns all validation errors collected
/// across all sections.
#[must_use = "this returns a Result that may indicate validation failure"]
pub fn validate_config(config: &AletheiaConfig) -> Result<(), ValidationError> {
    let value = serde_json::to_value(config).unwrap_or(Value::Null);
    let Value::Object(ref sections) = value else {
        return ValidationSnafu {
            errors: vec!["config did not serialize to a JSON object".to_owned()],
        }
        .fail();
    };

    let mut all_errors = Vec::new();
    for (section, val) in sections {
        if let Err(err) = validate_section(section, val) {
            for e in err.errors {
                all_errors.push(format!("{section}: {e}"));
            }
        }
    }

    if all_errors.is_empty() {
        Ok(())
    } else {
        ValidationSnafu { errors: all_errors }.fail()
    }
}

/// Validate a config section update.
///
/// # Errors
///
/// Returns [`ValidationError`] if any field value is out of range, empty when
/// required, or the section name is unrecognized. The error contains all
/// collected validation messages.
#[must_use = "this returns a Result that may indicate validation failure"]
pub fn validate_section(section: &str, value: &Value) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    match section {
        "agents" => validate_agents(value, &mut errors),
        "gateway" => validate_gateway(value, &mut errors),
        "maintenance" => validate_maintenance(value, &mut errors),
        "data" => validate_data(value, &mut errors),
        "embedding" => validate_embedding(value, &mut errors),
        "channels" => validate_channels(value, &mut errors),
        "bindings" => validate_bindings(value, &mut errors),
        "credential" => validate_credential(value, &mut errors),
        // NOTE: these sections are pass-through with no validation rules
        "packs" | "pricing" | "sandbox" | "logging" | "mcp" => {}
        _ => errors.push(format!("unknown config section: {section}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        ValidationSnafu { errors }.fail()
    }
}

/// Maximum allowed token budget for any single field.
const MAX_TOKEN_BUDGET: u64 = 1_000_000;

fn validate_agents(value: &Value, errors: &mut Vec<String>) {
    if let Some(defaults) = value.get("defaults") {
        check_positive_u32(defaults, "contextTokens", errors);
        check_positive_u32(defaults, "maxOutputTokens", errors);
        check_positive_u32(defaults, "bootstrapMaxTokens", errors);
        check_positive_u32(defaults, "timeoutSeconds", errors);
        check_positive_u32(defaults, "thinkingBudget", errors);

        // WHY: Cap token budgets at a sane maximum to prevent misconfiguration.
        for field in &[
            "contextTokens",
            "maxOutputTokens",
            "bootstrapMaxTokens",
            "thinkingBudget",
        ] {
            if let Some(val) = defaults.get(field).and_then(Value::as_u64)
                && val > MAX_TOKEN_BUDGET
            {
                errors.push(format!("{field} must not exceed {MAX_TOKEN_BUDGET} tokens"));
            }
        }

        if let Some(model) = defaults.get("model") {
            if let Some(primary) = model.get("primary").and_then(Value::as_str)
                && primary.is_empty()
            {
                errors.push("agents.defaults.model.primary must not be empty".to_owned());
            }
            if let Some(fallbacks) = model.get("fallbacks").and_then(Value::as_array) {
                for (i, fallback) in fallbacks.iter().enumerate() {
                    if let Some(s) = fallback.as_str()
                        && s.is_empty()
                    {
                        errors.push(format!(
                            "agents.defaults.model.fallbacks[{i}] must not be empty"
                        ));
                    }
                }
            }
        }

        if let Some(val) = defaults.get("maxToolIterations").and_then(Value::as_u64)
            && (val == 0 || val > 10_000)
        {
            errors.push("maxToolIterations must be between 1 and 10000".to_owned());
        }

        if let Some(agency) = defaults.get("agency").and_then(Value::as_str)
            && !matches!(agency, "unrestricted" | "standard" | "restricted")
        {
            errors.push(format!(
                "agency must be \"unrestricted\", \"standard\", or \"restricted\", got \"{agency}\""
            ));
        }

        if let Some(timeouts) = defaults.get("toolTimeouts")
            && let Some(val) = timeouts.get("defaultMs").and_then(Value::as_u64)
            && val == 0
        {
            errors.push("toolTimeouts.defaultMs must be positive".to_owned());
        }

        // INVARIANT: Bootstrap budget must fit within the context window.
        let context = defaults.get("contextTokens").and_then(Value::as_u64);
        let bootstrap = defaults.get("bootstrapMaxTokens").and_then(Value::as_u64);
        if let (Some(ctx), Some(boot)) = (context, bootstrap)
            && boot >= ctx
        {
            errors.push(format!(
                "bootstrapMaxTokens ({boot}) must be less than contextTokens ({ctx})"
            ));
        }
    }
}

const VALID_AUTH_MODES: &[&str] = &["none", "token", "jwt"];

fn validate_gateway(value: &Value, errors: &mut Vec<String>) {
    if let Some(port) = value.get("port").and_then(Value::as_u64)
        && (port == 0 || port > 65535)
    {
        errors.push("port must be between 1 and 65535".to_owned());
    }

    if let Some(auth) = value.get("auth")
        && let Some(mode) = auth.get("mode").and_then(Value::as_str)
        && !VALID_AUTH_MODES.contains(&mode)
    {
        errors.push(format!(
            "gateway.auth.mode '{mode}' is invalid; must be one of: none, token, jwt"
        ));
    }

    if let Some(cors) = value.get("cors")
        && let Some(val) = cors.get("maxAgeSecs").and_then(Value::as_u64)
        && val == 0
    {
        errors.push("cors.maxAgeSecs must be positive".to_owned());
    }

    if let Some(body_limit) = value.get("bodyLimit")
        && let Some(val) = body_limit.get("maxBytes").and_then(Value::as_u64)
        && val == 0
    {
        errors.push("bodyLimit.maxBytes must be positive".to_owned());
    }
}

fn validate_maintenance(value: &Value, errors: &mut Vec<String>) {
    if let Some(tr) = value.get("traceRotation") {
        check_positive_u32(tr, "maxAgeDays", errors);
        if let Some(val) = tr.get("maxTotalSizeMb").and_then(Value::as_u64)
            && val == 0
        {
            errors.push("traceRotation.maxTotalSizeMb must be positive".to_owned());
        }
    }

    if let Some(db) = value.get("dbMonitoring") {
        let warn = db.get("warnThresholdMb").and_then(Value::as_u64);
        let alert = db.get("alertThresholdMb").and_then(Value::as_u64);
        if let (Some(w), Some(a)) = (warn, alert)
            && w > a
        {
            errors.push("dbMonitoring.warnThresholdMb must not exceed alertThresholdMb".to_owned());
        }
    }
}

fn validate_data(value: &Value, errors: &mut Vec<String>) {
    if let Some(retention) = value.get("retention") {
        check_positive_u32(retention, "sessionMaxAgeDays", errors);
        check_positive_u32(retention, "orphanMessageMaxAgeDays", errors);
    }
}

fn validate_embedding(value: &Value, errors: &mut Vec<String>) {
    if let Some(provider) = value.get("provider").and_then(Value::as_str)
        && provider.is_empty()
    {
        errors.push("embedding.provider must not be empty".to_owned());
    }

    if let Some(dim) = value.get("dimension").and_then(Value::as_u64)
        && dim == 0
    {
        errors.push("embedding.dimension must be positive".to_owned());
    }
}

fn validate_channels(value: &Value, errors: &mut Vec<String>) {
    if let Some(signal) = value.get("signal")
        && let Some(accounts) = signal.get("accounts").and_then(Value::as_object)
    {
        for (account_id, account) in accounts {
            if let Some(port) = account.get("httpPort").and_then(Value::as_u64)
                && (port == 0 || port > 65535)
            {
                errors.push(format!(
                    "channels.signal.accounts.{account_id}.httpPort must be between 1 and 65535"
                ));
            }
        }
    }
}

fn validate_bindings(value: &Value, errors: &mut Vec<String>) {
    let Some(bindings) = value.as_array() else {
        return;
    };

    for (i, binding) in bindings.iter().enumerate() {
        for field in &["channel", "source", "nousId"] {
            match binding.get(field).and_then(Value::as_str) {
                None | Some("") => {
                    errors.push(format!("bindings[{i}].{field} must not be empty"));
                }
                // NOTE: non-empty field value passes validation
                _ => {}
            }
        }
    }
}

fn validate_credential(value: &Value, errors: &mut Vec<String>) {
    if let Some(source) = value.get("source").and_then(Value::as_str)
        && !matches!(source, "auto" | "api-key" | "claude-code")
    {
        errors.push(format!(
            "credential.source must be \"auto\", \"api-key\", or \"claude-code\", got \"{source}\""
        ));
    }
}

fn check_positive_u32(parent: &Value, key: &str, errors: &mut Vec<String>) {
    if let Some(val) = parent.get(key) {
        if let Some(n) = val.as_u64() {
            if n == 0 {
                errors.push(format!("{key} must be positive"));
            }
        } else if let Some(n) = val.as_i64()
            && n <= 0
        {
            errors.push(format!("{key} must be positive"));
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_zero_timeout() {
        let section = json!({ "defaults": { "timeoutSeconds": 0 } });
        let result = validate_section("agents", &section);
        assert!(result.is_err());
        assert!(result.unwrap_err().errors[0].contains("timeoutSeconds"));
    }

    #[test]
    fn rejects_excessive_tool_iterations() {
        let section = json!({ "defaults": { "maxToolIterations": 10_001 } });
        let result = validate_section("agents", &section);
        assert!(result.is_err());
    }

    #[test]
    fn accepts_tool_iterations_within_unrestricted_range() {
        let section = json!({ "defaults": { "maxToolIterations": 10_000 } });
        assert!(validate_section("agents", &section).is_ok());
    }

    #[test]
    fn rejects_invalid_port() {
        let section = json!({ "port": 0 });
        let result = validate_section("gateway", &section);
        assert!(result.is_err());

        let section = json!({ "port": 70000 });
        let result = validate_section("gateway", &section);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_warn_exceeding_alert() {
        let section = json!({
            "dbMonitoring": { "warnThresholdMb": 500, "alertThresholdMb": 100 }
        });
        let result = validate_section("maintenance", &section);
        assert!(result.is_err());
    }

    #[test]
    fn accepts_valid_agents() {
        let section = json!({
            "defaults": {
                "contextTokens": 200_000,
                "timeoutSeconds": 300,
                "maxToolIterations": 200,
                "thinkingBudget": 10_000
            }
        });
        assert!(validate_section("agents", &section).is_ok());
    }

    #[test]
    fn accepts_valid_gateway() {
        let section = json!({ "port": 8080, "cors": { "maxAgeSecs": 3600 } });
        assert!(validate_section("gateway", &section).is_ok());
    }

    #[test]
    fn unknown_section_errors() {
        let result = validate_section("nonexistent", &json!({}));
        assert!(result.is_err());
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
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.errors[0].contains("bootstrapMaxTokens"));
    }

    #[test]
    fn accepts_bootstrap_within_context() {
        let section = json!({
            "defaults": {
                "contextTokens": 200_000,
                "bootstrapMaxTokens": 40_000
            }
        });
        assert!(validate_section("agents", &section).is_ok());
    }

    #[test]
    fn rejects_invalid_auth_mode() {
        let section = json!({ "auth": { "mode": "magic" } });
        let result = validate_section("gateway", &section);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.errors[0].contains("gateway.auth.mode"));
    }

    #[test]
    fn accepts_valid_auth_modes() {
        for mode in &["none", "token", "jwt"] {
            let section = json!({ "auth": { "mode": mode } });
            assert!(
                validate_section("gateway", &section).is_ok(),
                "mode '{mode}' should be valid"
            );
        }
    }

    #[test]
    fn rejects_zero_embedding_dimension() {
        let section = json!({ "dimension": 0 });
        let result = validate_section("embedding", &section);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_empty_embedding_provider() {
        let section = json!({ "provider": "" });
        let result = validate_section("embedding", &section);
        assert!(result.is_err());
    }

    #[test]
    fn accepts_valid_embedding() {
        let section = json!({ "provider": "candle", "dimension": 384 });
        assert!(validate_section("embedding", &section).is_ok());
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
        assert!(result.is_err());
    }

    #[test]
    fn accepts_valid_channels() {
        let section = json!({
            "signal": {
                "accounts": {
                    "primary": { "httpPort": 8080 }
                }
            }
        });
        assert!(validate_section("channels", &section).is_ok());
    }

    #[test]
    fn rejects_binding_with_empty_fields() {
        let section = json!([
            { "channel": "signal", "source": "*", "nousId": "" }
        ]);
        let result = validate_section("bindings", &section);
        assert!(result.is_err());
    }

    #[test]
    fn accepts_valid_bindings() {
        let section = json!([
            { "channel": "signal", "source": "*", "nousId": "main" }
        ]);
        assert!(validate_section("bindings", &section).is_ok());
    }

    #[test]
    fn rejects_invalid_credential_source() {
        let section = json!({ "source": "magic" });
        let result = validate_section("credential", &section);
        assert!(result.is_err());
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
        assert!(result.is_err());
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
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.errors.iter().any(|e| e.contains("model.primary")),
            "expected model.primary error, got: {err:?}"
        );
    }

    #[test]
    fn rejects_empty_model_fallback() {
        let section = json!({ "defaults": { "model": { "primary": "claude-sonnet-4-6", "fallbacks": [""] } } });
        let result = validate_section("agents", &section);
        assert!(result.is_err());
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
        assert!(validate_section("agents", &section).is_ok());
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
        assert!(validate_section("agents", &section).is_ok());
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
}
