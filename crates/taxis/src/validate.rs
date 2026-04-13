//! Config section validation: rejects invalid values before persisting.

use serde_json::Value;
use snafu::Snafu;

use crate::config::AletheiaConfig;
use crate::oikos::Oikos;

/// Validation error with collected messages.
#[derive(Debug, Snafu)]
#[snafu(display("config validation failed:\n  - {}", errors.join("\n  - ")))]
pub struct ValidationError {
    /// Collected validation error messages.
    pub errors: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}

/// Validate an entire [`AletheiaConfig`] by checking each section.
///
/// Serializes to JSON and validates each top-level section using
/// [`validate_section`]. Returns all validation errors collected
/// across all sections.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub(crate) fn validate_config(config: &AletheiaConfig) -> Result<(), ValidationError> {
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

/// Validate config preconditions required before server startup.
///
/// Checks that at least one agent is defined and that every agent's workspace
/// directory exists on disk. Call this after config loading but before actors
/// spawn.
///
/// # Errors
///
/// Returns [`ValidationError`] if `agents.list` is empty or any agent's
/// workspace directory does not exist.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
pub fn validate_startup(config: &AletheiaConfig, oikos: &Oikos) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    if config.agents.list.is_empty() {
        errors.push("agents.list is empty: at least one agent must be configured".to_owned());
    }

    for agent in &config.agents.list {
        let path = if std::path::Path::new(&agent.workspace).is_absolute() {
            std::path::PathBuf::from(&agent.workspace)
        } else {
            oikos.root().join(&agent.workspace)
        };
        if !path.is_dir() {
            errors.push(format!(
                "agent '{}' workspace '{}' does not exist",
                agent.id, agent.workspace
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        ValidationSnafu { errors }.fail()
    }
}

/// Validate a config section update.
///
/// # Errors
///
/// Returns [`ValidationError`] if any field value is out of range, empty when
/// required, or the section name is unrecognized. The error contains all
/// collected validation messages.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
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
        "timeouts" => validate_timeouts(value, &mut errors),
        "capacity" => validate_capacity(value, &mut errors),
        "retry" => validate_retry(value, &mut errors),
        "nousBehavior" => validate_nous_behavior(value, &mut errors),
        "knowledge" => validate_knowledge(value, &mut errors),
        "providerBehavior" => validate_provider_behavior(value, &mut errors),
        "apiLimits" => validate_api_limits(value, &mut errors),
        "daemonBehavior" => validate_daemon_behavior(value, &mut errors),
        "toolLimits" => validate_tool_limits(value, &mut errors),
        "messaging" => validate_messaging(value, &mut errors),
        "tuning" => validate_tuning(value, &mut errors),
        // NOTE: pass-through sections with no validation rules.
        "packs" | "pricing" | "sandbox" | "logging" | "mcp" | "localProvider" | "training" => {}
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
    check_port(value, "port", "port", errors);

    if let Some(auth) = value.get("auth")
        && let Some(mode) = auth.get("mode").and_then(Value::as_str)
        && !VALID_AUTH_MODES.contains(&mode)
    {
        errors.push(format!(
            "gateway.auth.mode '{mode}' is invalid; must be one of: none, token, jwt"
        ));
    }

    if let Some(cors) = value.get("cors") {
        check_positive_u64(cors, "maxAgeSecs", errors);
    }

    if let Some(body_limit) = value.get("bodyLimit") {
        check_positive_u64(body_limit, "maxBytes", errors);
    }
}

fn validate_maintenance(value: &Value, errors: &mut Vec<String>) {
    if let Some(tr) = value.get("traceRotation") {
        check_positive_u32(tr, "maxAgeDays", errors);
        check_positive_u64(tr, "maxTotalSizeMb", errors);
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

    check_positive_u64(value, "dimension", errors);
}

fn validate_channels(value: &Value, errors: &mut Vec<String>) {
    if let Some(signal) = value.get("signal")
        && let Some(accounts) = signal.get("accounts").and_then(Value::as_object)
    {
        for (account_id, account) in accounts {
            check_port(
                account,
                "httpPort",
                &format!("channels.signal.accounts.{account_id}.httpPort"),
                errors,
            );
        }
    }
}

const KNOWN_CHANNEL_TYPES: &[&str] = &["signal", "slack", "discord", "webhook", "http"];

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
                _ => {
                    // NOTE: non-empty field value passes validation
                }
            }
        }

        if let Some(channel) = binding.get("channel").and_then(Value::as_str)
            && !channel.is_empty()
            && !KNOWN_CHANNEL_TYPES.contains(&channel)
        {
            errors.push(format!(
                "bindings[{i}].channel '{channel}' is not a known channel type (expected one of: {})",
                KNOWN_CHANNEL_TYPES.join(", ")
            ));
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

fn validate_timeouts(value: &Value, errors: &mut Vec<String>) {
    // WHY: 30s minimum prevents misconfiguration that would time out before
    // any model response arrives. 3600s cap prevents runaway session budgets.
    if let Some(val) = value.get("llmCallSecs").and_then(Value::as_u64) {
        if val < 30 {
            errors.push("timeouts.llmCallSecs must be at least 30 seconds".to_owned());
        }
        if val > 3600 {
            errors.push("timeouts.llmCallSecs must not exceed 3600 seconds".to_owned());
        }
    }
}

fn validate_capacity(value: &Value, errors: &mut Vec<String>) {
    // WHY: zero disables truncation (valid), but values above 10 MiB are
    // likely misconfiguration that would OOM tool result buffers.
    const MAX_TOOL_OUTPUT_BYTES: u64 = 10 * 1024 * 1024;
    if let Some(val) = value.get("maxToolOutputBytes").and_then(Value::as_u64)
        && val > MAX_TOOL_OUTPUT_BYTES
    {
        errors.push(format!(
            "capacity.maxToolOutputBytes must not exceed {MAX_TOOL_OUTPUT_BYTES} bytes (10 MiB)"
        ));
    }

    // WHY: opus_context_tokens must be at least 200k (the standard default) and
    // no more than 2M to prevent nonsensical configurations.
    if let Some(val) = value.get("opusContextTokens").and_then(Value::as_u64) {
        if val < 200_000 {
            errors.push(
                "capacity.opusContextTokens must be at least 200 000 tokens".to_owned(),
            );
        }
        if val > 2_000_000 {
            errors.push(
                "capacity.opusContextTokens must not exceed 2 000 000 tokens".to_owned(),
            );
        }
    }
}

fn validate_retry(value: &Value, errors: &mut Vec<String>) {
    // WHY: cap ensures callers never stall for more than 5 minutes.
    const MAX_BACKOFF_MS: u64 = 300_000;

    // WHY: cap at 10 retries to prevent runaway loops on persistent failures.
    if let Some(val) = value.get("maxAttempts").and_then(Value::as_u64)
        && val > 10
    {
        errors.push("retry.maxAttempts must not exceed 10".to_owned());
    }

    // WHY: 100ms minimum prevents busy-looping under rapid failures.
    if let Some(val) = value.get("backoffBaseMs").and_then(Value::as_u64)
        && val < 100
    {
        errors.push("retry.backoffBaseMs must be at least 100 ms".to_owned());
    }

    if let Some(val) = value.get("backoffMaxMs").and_then(Value::as_u64)
        && val > MAX_BACKOFF_MS
    {
        errors.push(format!(
            "retry.backoffMaxMs must not exceed {MAX_BACKOFF_MS} ms (5 minutes)"
        ));
    }

    // INVARIANT: max must be >= base so the cap is reachable.
    let base = value.get("backoffBaseMs").and_then(Value::as_u64);
    let max = value.get("backoffMaxMs").and_then(Value::as_u64);
    if let (Some(b), Some(m)) = (base, max)
        && m < b
    {
        errors.push(
            "retry.backoffMaxMs must be greater than or equal to backoffBaseMs".to_owned(),
        );
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

/// Reject a u64 field if its value is zero.
fn check_positive_u64(parent: &Value, key: &str, errors: &mut Vec<String>) {
    if let Some(val) = parent.get(key).and_then(Value::as_u64)
        && val == 0
    {
        errors.push(format!("{key} must be positive"));
    }
}

/// Reject a u64 field if its value is outside the port range (1..=65535).
fn check_port(parent: &Value, key: &str, label: &str, errors: &mut Vec<String>) {
    if let Some(port) = parent.get(key).and_then(Value::as_u64)
        && (port == 0 || port > 65535)
    {
        errors.push(format!("{label} must be between 1 and 65535"));
    }
}

/// Reject a numeric field if its value is outside `[min, max]`.
fn check_range_f64(parent: &Value, key: &str, min: f64, max: f64, errors: &mut Vec<String>) {
    if let Some(val) = parent.get(key).and_then(Value::as_f64)
        && (val < min || val > max)
    {
        errors.push(format!("{key} must be between {min} and {max}, got {val}"));
    }
}

/// Reject a u64 field if its value is outside `[min, max]`.
fn check_range_u64(parent: &Value, key: &str, min: u64, max: u64, errors: &mut Vec<String>) {
    if let Some(val) = parent.get(key).and_then(Value::as_u64)
        && (val < min || val > max)
    {
        errors.push(format!("{key} must be between {min} and {max}, got {val}"));
    }
}

fn validate_nous_behavior(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "degradedPanicThreshold", 1, 100, errors);
    check_range_u64(value, "degradedWindowSecs", 1, 86_400, errors);
    check_range_u64(value, "inboxRecvTimeoutSecs", 1, 3600, errors);
    check_range_u64(value, "inboxCapacity", 1, 10_000, errors);
    check_range_u64(value, "maxSpawnedTasks", 1, 1_000, errors);
    check_range_u64(value, "maxSessions", 1, 100_000, errors);
    check_range_u64(value, "gcIntervalSecs", 30, 3600, errors);
    check_range_u64(value, "loopDetectionWindow", 5, 500, errors);
    check_range_u64(value, "cycleDetectionMaxLen", 1, 100, errors);
    check_range_u64(value, "selfAuditEventThreshold", 1, 10_000, errors);
    check_range_u64(value, "managerHealthIntervalSecs", 5, 300, errors);
    check_range_u64(value, "managerPingTimeoutSecs", 1, 60, errors);
    check_range_u64(value, "managerMaxRestartBackoffSecs", 1, 86_400, errors);
    check_range_u64(value, "managerRestartDrainTimeoutSecs", 1, 600, errors);
    check_range_u64(value, "managerRestartDecayWindowSecs", 60, 86_400, errors);
}

fn validate_knowledge(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "conflictMaxLlmCallsPerFact", 1, 20, errors);
    check_range_f64(
        value,
        "conflictIntraBatchDedupThreshold",
        0.5,
        1.0,
        errors,
    );
    check_range_f64(
        value,
        "conflictCandidateDistanceThreshold",
        0.01,
        1.0,
        errors,
    );
    check_range_u64(value, "conflictMaxCandidates", 1, 50, errors);
    check_range_f64(value, "decayReinforcementBoost", 0.001, 1.0, errors);
    check_range_f64(value, "decayMaxReinforcementBonus", 0.0, 10.0, errors);
    check_range_f64(value, "decayCrossAgentBonusPerAgent", 0.0, 1.0, errors);
    check_range_f64(value, "decayMaxCrossAgentMultiplier", 1.0, 10.0, errors);
    check_range_f64(value, "extractionConfidenceThreshold", 0.0, 1.0, errors);
    check_range_u64(value, "extractionMinFactLength", 1, 1000, errors);
    check_range_u64(value, "extractionMaxFactLength", 10, 10_000, errors);

    // INVARIANT: min length must be less than max length.
    let min_len = value
        .get("extractionMinFactLength")
        .and_then(Value::as_u64);
    let max_len = value
        .get("extractionMaxFactLength")
        .and_then(Value::as_u64);
    if let (Some(min), Some(max)) = (min_len, max_len)
        && min > max
    {
        errors.push(format!(
            "extractionMinFactLength ({min}) must not exceed extractionMaxFactLength ({max})"
        ));
    }
}

fn validate_provider_behavior(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "nonStreamingTimeoutSecs", 10, 600, errors);
    check_range_u64(value, "sseDefaultRetryMs", 100, 60_000, errors);
    check_range_f64(value, "concurrencyEwmaAlpha", 0.0, 1.0, errors);
    check_range_f64(
        value,
        "concurrencyLatencyThresholdSecs",
        1.0,
        300.0,
        errors,
    );
    check_range_u64(value, "complexityLowThreshold", 0, 100, errors);
    check_range_u64(value, "complexityHighThreshold", 0, 100, errors);

    // INVARIANT: low threshold must be <= high threshold.
    let low = value
        .get("complexityLowThreshold")
        .and_then(Value::as_u64);
    let high = value
        .get("complexityHighThreshold")
        .and_then(Value::as_u64);
    if let (Some(l), Some(h)) = (low, high)
        && l > h
    {
        errors.push(format!(
            "complexityLowThreshold ({l}) must not exceed complexityHighThreshold ({h})"
        ));
    }
}

fn validate_api_limits(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "maxSessionNameLen", 1, 10_000, errors);
    check_range_u64(value, "maxIdentifierBytes", 1, 10_000, errors);
    check_range_u64(value, "maxHistoryLimit", 1, 100_000, errors);
    check_range_u64(value, "defaultHistoryLimit", 1, 100_000, errors);
    check_range_u64(value, "maxMessageBytes", 1024, 104_857_600, errors);
    check_range_u64(value, "maxFactsLimit", 1, 100_000, errors);
    check_range_u64(value, "maxSearchLimit", 1, 100_000, errors);
    check_range_u64(value, "maxImportBatchSize", 1, 100_000, errors);
    check_range_u64(value, "idempotencyTtlSecs", 10, 86_400, errors);
    check_range_u64(value, "idempotencyCapacity", 100, 10_000_000, errors);
    check_range_u64(value, "idempotencyMaxKeyLength", 1, 1024, errors);

    // INVARIANT: default history limit must be <= max history limit.
    let default_limit = value
        .get("defaultHistoryLimit")
        .and_then(Value::as_u64);
    let max_limit = value
        .get("maxHistoryLimit")
        .and_then(Value::as_u64);
    if let (Some(d), Some(m)) = (default_limit, max_limit)
        && d > m
    {
        errors.push(format!(
            "defaultHistoryLimit ({d}) must not exceed maxHistoryLimit ({m})"
        ));
    }
}

fn validate_daemon_behavior(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "watchdogBackoffBaseSecs", 1, 60, errors);
    check_range_u64(value, "watchdogBackoffCapSecs", 10, 3600, errors);
    check_range_u64(value, "prosocheAnomalySampleSize", 2, 1000, errors);
    check_range_u64(value, "runnerOutputBriefHeadLines", 1, 100, errors);
    check_range_u64(value, "runnerOutputBriefTailLines", 1, 100, errors);

    // INVARIANT: backoff base must be <= backoff cap.
    let base = value
        .get("watchdogBackoffBaseSecs")
        .and_then(Value::as_u64);
    let cap = value.get("watchdogBackoffCapSecs").and_then(Value::as_u64);
    if let (Some(b), Some(c)) = (base, cap)
        && b > c
    {
        errors.push(format!(
            "watchdogBackoffBaseSecs ({b}) must not exceed watchdogBackoffCapSecs ({c})"
        ));
    }
}

fn validate_tool_limits(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "maxPatternLength", 10, 100_000, errors);
    check_range_u64(value, "subprocessTimeoutSecs", 5, 600, errors);
    check_range_u64(value, "maxWriteBytes", 1024, 1_073_741_824, errors);
    check_range_u64(value, "maxReadBytes", 1024, 1_073_741_824, errors);
    check_range_u64(value, "maxCommandLength", 100, 1_000_000, errors);
    check_range_u64(value, "messageMaxLen", 100, 1_000_000, errors);
    check_range_u64(value, "interSessionMaxMessageLen", 1000, 10_000_000, errors);
    check_range_u64(value, "interSessionMaxTimeoutSecs", 10, 3600, errors);
}

fn validate_messaging(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "pollIntervalMs", 100, 60_000, errors);
    check_range_u64(value, "bufferCapacity", 1, 100_000, errors);
    check_range_u64(value, "circuitBreakerThreshold", 1, 100, errors);
    check_range_u64(value, "haltedHealthCheckIntervalSecs", 1, 3600, errors);
    check_range_u64(value, "rpcTimeoutSecs", 1, 300, errors);
    check_range_u64(value, "healthTimeoutSecs", 1, 60, errors);
    check_range_u64(value, "receiveTimeoutSecs", 1, 300, errors);
    check_range_u64(value, "agentDispatchTimeoutSecs", 10, 3600, errors);
    check_range_u64(value, "maxConcurrentHandlers", 1, 10_000, errors);
}

fn validate_tuning(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "maxChangesPerCycle", 1, 20, errors);
    check_range_u64(value, "evidenceMinSamples", 2, 1000, errors);
    if let Some(threshold) = value.get("significanceThreshold").and_then(Value::as_f64)
        && !(0.1..=10.0).contains(&threshold)
    {
        errors.push(format!(
            "tuning.significanceThreshold must be between 0.1 and 10.0, got {threshold}"
        ));
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec[0] is valid after asserting errors are non-empty"
)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rejects_zero_timeout() {
        let section = json!({ "defaults": { "timeoutSeconds": 0 } });
        let result = validate_section("agents", &section);
        assert!(result.is_err(), "zero timeoutSeconds should be rejected");
        assert!(
            result.unwrap_err().errors[0].contains("timeoutSeconds"),
            "error should mention timeoutSeconds"
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
                "timeoutSeconds": 300,
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
            }
        });
        assert!(
            validate_section("channels", &section).is_ok(),
            "valid channel config should be accepted"
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
        let section = json!({ "defaults": { "model": { "primary": "claude-sonnet-4-6", "fallbacks": [""] } } });
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

    // --- Wave 5: behavioral section validators ---

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
            "maxMessageBytes": 262144,
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
            "maxWriteBytes": 10485760
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
}
