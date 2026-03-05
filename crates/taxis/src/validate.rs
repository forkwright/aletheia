//! Config section validation — rejects invalid values before persisting.

use serde_json::Value;

/// Validate a config section update. Returns errors for invalid values.
pub fn validate_section(section: &str, value: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    match section {
        "agents" => validate_agents(value, &mut errors),
        "gateway" => validate_gateway(value, &mut errors),
        "maintenance" => validate_maintenance(value, &mut errors),
        "data" => validate_data(value, &mut errors),
        "embedding" | "channels" | "bindings" | "packs" | "pricing" => {}
        _ => errors.push(format!("unknown config section: {section}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_agents(value: &Value, errors: &mut Vec<String>) {
    if let Some(defaults) = value.get("defaults") {
        check_positive_u32(defaults, "contextTokens", errors);
        check_positive_u32(defaults, "maxOutputTokens", errors);
        check_positive_u32(defaults, "bootstrapMaxTokens", errors);
        check_positive_u32(defaults, "timeoutSeconds", errors);
        check_positive_u32(defaults, "thinkingBudget", errors);

        if let Some(val) = defaults.get("maxToolIterations").and_then(Value::as_u64) {
            if val == 0 || val > 200 {
                errors.push("maxToolIterations must be between 1 and 200".to_owned());
            }
        }

        if let Some(timeouts) = defaults.get("toolTimeouts") {
            if let Some(val) = timeouts.get("defaultMs").and_then(Value::as_u64) {
                if val == 0 {
                    errors.push("toolTimeouts.defaultMs must be positive".to_owned());
                }
            }
        }
    }
}

fn validate_gateway(value: &Value, errors: &mut Vec<String>) {
    if let Some(port) = value.get("port").and_then(Value::as_u64) {
        if port == 0 || port > 65535 {
            errors.push("port must be between 1 and 65535".to_owned());
        }
    }

    if let Some(cors) = value.get("cors") {
        if let Some(val) = cors.get("maxAgeSecs").and_then(Value::as_u64) {
            if val == 0 {
                errors.push("cors.maxAgeSecs must be positive".to_owned());
            }
        }
    }

    if let Some(body_limit) = value.get("bodyLimit") {
        if let Some(val) = body_limit.get("maxBytes").and_then(Value::as_u64) {
            if val == 0 {
                errors.push("bodyLimit.maxBytes must be positive".to_owned());
            }
        }
    }
}

fn validate_maintenance(value: &Value, errors: &mut Vec<String>) {
    if let Some(tr) = value.get("traceRotation") {
        check_positive_u32(tr, "maxAgeDays", errors);
        if let Some(val) = tr.get("maxTotalSizeMb").and_then(Value::as_u64) {
            if val == 0 {
                errors.push("traceRotation.maxTotalSizeMb must be positive".to_owned());
            }
        }
    }

    if let Some(db) = value.get("dbMonitoring") {
        let warn = db.get("warnThresholdMb").and_then(Value::as_u64);
        let alert = db.get("alertThresholdMb").and_then(Value::as_u64);
        if let (Some(w), Some(a)) = (warn, alert) {
            if w > a {
                errors
                    .push("dbMonitoring.warnThresholdMb must not exceed alertThresholdMb".to_owned());
            }
        }
    }
}

fn validate_data(value: &Value, errors: &mut Vec<String>) {
    if let Some(retention) = value.get("retention") {
        check_positive_u32(retention, "sessionMaxAgeDays", errors);
        check_positive_u32(retention, "orphanMessageMaxAgeDays", errors);
    }
}

fn check_positive_u32(parent: &Value, key: &str, errors: &mut Vec<String>) {
    if let Some(val) = parent.get(key) {
        if let Some(n) = val.as_u64() {
            if n == 0 {
                errors.push(format!("{key} must be positive"));
            }
        } else if let Some(n) = val.as_i64() {
            if n <= 0 {
                errors.push(format!("{key} must be positive"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_zero_timeout() {
        let section = json!({ "defaults": { "timeoutSeconds": 0 } });
        let result = validate_section("agents", &section);
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].contains("timeoutSeconds"));
    }

    #[test]
    fn rejects_excessive_tool_iterations() {
        let section = json!({ "defaults": { "maxToolIterations": 300 } });
        let result = validate_section("agents", &section);
        assert!(result.is_err());
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
                "maxToolIterations": 50,
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
}
