// kanon:ignore RUST/file-too-long — config section validators are discrete functions naturally colocated with their dispatch table
//! Config section validation: rejects invalid values before persisting.

use std::collections::HashSet;

use koina::id::ToolName;
use serde_json::Value;
use snafu::Snafu;

use crate::config::AletheiaConfig;
use crate::oikos::Oikos;
use crate::workspace_schema::validate_agent_workspaces;

/// Validation error with collected messages.
#[derive(Debug, Snafu)]
#[snafu(display("config validation failed:\n  - {}", errors.join("\n  - ")))]
// kanon:ignore TOPOLOGY/shallow-struct — snafu derive provides display and Error impl; pub fields are the error surface
pub struct ValidationError {
    /// Collected validation error messages.
    pub errors: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub(crate) location: snafu::Location,
}

impl ValidationError {
    /// Returns the collected validation error messages.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
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
// kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
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
// kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
pub fn validate_startup(config: &AletheiaConfig, oikos: &Oikos) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    // WHY: Instance subdirectories are checked first because missing layout
    // causes runtime failures (e.g. first write to data/ fails).
    for subdir in REQUIRED_INSTANCE_SUBDIRS {
        let path = oikos.root().join(subdir);
        if !path.is_dir() {
            errors.push(format!(
                "required instance directory '{}' does not exist\n  \
                 help: run `aletheia init` to create the instance layout",
                path.display()
            ));
        }
    }

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

    if let Err(err) = validate_agent_workspaces(config, oikos) {
        errors.extend(err.failures);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        ValidationSnafu { errors }.fail()
    }
}

/// Instance subdirectories required for correct runtime operation.
///
/// Missing any of these causes failures at first use (e.g. database writes
/// to data/, log file creation in logs/, agent workspace loading from nous/).
const REQUIRED_INSTANCE_SUBDIRS: &[&str] = &["config", "data", "nous"];

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
// kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
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
        "feature_flags" => validate_feature_flags(value, &mut errors),
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
        "jwt" => validate_jwt(value, &mut errors),
        "providers" => validate_providers(value, &mut errors),
        "tools" => validate_tools(value, &mut errors),
        // NOTE: pass-through sections with no validation rules.
        "packs" | "pricing" | "sandbox" | "logging" | "observability" | "mcp" | "localProvider"
        | "training" | "anthropic" | "promptAudit" | "dispatch" | "workspace" => {}
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
            if let Some(primary) = model.get("primary") {
                validate_model_route(primary, "agents.defaults.model.primary", errors);
            }
            if let Some(fallbacks) = model.get("fallbacks").and_then(Value::as_array) {
                for (i, fallback) in fallbacks.iter().enumerate() {
                    validate_model_route(
                        fallback,
                        &format!("agents.defaults.model.fallbacks[{i}]"),
                        errors,
                    );
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

fn validate_model_route(value: &Value, path: &str, errors: &mut Vec<String>) {
    match value {
        Value::String(model) if model.is_empty() => {
            errors.push(format!("{path} must not be empty"));
        }
        Value::Object(fields) => {
            for field in fields.keys() {
                if field != "model" && field != "provider" {
                    errors.push(format!(
                        "{path}.{field} is not a recognized model route field"
                    ));
                }
            }
            match fields.get("model").and_then(Value::as_str) {
                Some("") => {
                    errors.push(format!("{path}.model must not be empty"));
                }
                None => {
                    errors.push(format!("{path}.model must be a string"));
                }
                Some(_) => {}
            }
            if let Some(provider) = fields.get("provider").and_then(Value::as_str)
                && provider.is_empty()
            {
                errors.push(format!("{path}.provider must not be empty"));
            }
        }
        _ => {
            // NOTE: non-object route values have no model-route fields to validate.
        }
    }
}

const VALID_AUTH_MODES: &[&str] = &["none", "token", "jwt"];

/// Environment variable operators must set to `1` in order to accept a config
/// write that sets `gateway.auth.mode = "none"`.
///
/// WHY: Disabling authentication removes all access control from the HTTP API.
/// Requiring an explicit opt-in prevents a remote PUT /api/v1/config/gateway
/// from silently turning off auth. (#3383)
pub const ALLOW_AUTH_NONE_ENV: &str = "ALETHEIA_ALLOW_AUTH_NONE";

/// Environment variable operators must set to `1` to allow the server to bind
/// to a non-localhost address while running with `gateway.auth.mode = "none"`.
///
/// WHY: disabled-auth on localhost is a supported local-dev shape; disabled-auth
/// on a LAN or Tailscale bind is an insecure-by-default posture we refuse to
/// boot into. Operators who genuinely want unauthenticated LAN access must
/// flip this knob explicitly. The variable is distinct from
/// [`ALLOW_AUTH_NONE_ENV`] because disabling auth locally is a meaningfully
/// smaller blast radius than disabling auth and exposing it to the tailnet.
/// (#3716)
pub const ALLOW_AUTH_NONE_LAN_ENV: &str = "ALETHEIA_ALLOW_AUTH_NONE_LAN";

/// `true` when the bound address is a loopback address that keeps the API
/// reachable only from the local machine.
#[must_use]
pub fn is_loopback_bind(addr: &str) -> bool {
    matches!(addr, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// Return `true` when the operator has set `ALETHEIA_ALLOW_AUTH_NONE_LAN=1`,
/// which is required to boot the server with `auth.mode = "none"` on a
/// non-localhost bind.
#[must_use]
pub fn auth_none_lan_opt_in_enabled() -> bool {
    std::env::var(ALLOW_AUTH_NONE_LAN_ENV).is_ok_and(|v| v == "1")
}

fn validate_gateway(value: &Value, errors: &mut Vec<String>) {
    check_port(value, "port", "port", errors);

    // WHY: structural validity only — accept any of the three known auth modes.
    // The `auth.mode = "none"` policy gate (env-var opt-in) lives in
    // [`validate_auth_mode_policy`] so it fires only when a config PUT through
    // the config API attempts to disable auth, not when a TOML file containing
    // `mode = "none"` is loaded at startup or inspected by `check-config`.
    // Operators have filesystem-level control of the file; the loud startup
    // warning emitted by [`warn_if_auth_disabled`] handles operator visibility.
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

    if let Some(csrf) = value.get("csrf") {
        let enabled = csrf.get("enabled").and_then(Value::as_bool);
        let disable_acknowledged = csrf
            .get("disableAcknowledged")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if enabled == Some(false) && !disable_acknowledged {
            errors.push(
                "gateway.csrf.enabled = false requires gateway.csrf.disableAcknowledged = true"
                    .to_owned(),
            );
        }

        if let Some(header_name) = csrf.get("headerName").and_then(Value::as_str)
            && header_name.trim().is_empty()
        {
            errors.push("gateway.csrf.headerName must not be empty".to_owned());
        }

        if let Some(header_value) = csrf.get("headerValue").and_then(Value::as_str)
            && header_value.is_empty()
        {
            errors.push("gateway.csrf.headerValue must not be empty".to_owned());
        }
    }

    if let Some(rate_limit) = value.get("rateLimit") {
        let enabled = rate_limit
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if enabled {
            check_positive_u32(rate_limit, "requestsPerMinute", errors);
        }

        let trust_proxy = rate_limit
            .get("trustProxy")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if trust_proxy && !enabled {
            errors.push(
                "gateway.rateLimit.trustProxy = true requires gateway.rateLimit.enabled = true"
                    .to_owned(),
            );
        }
    }
}

/// Reject `gateway.auth.mode = "none"` unless the operator has set
/// `ALETHEIA_ALLOW_AUTH_NONE=1`. This is the config-API-level gate that
/// prevents a config PUT from silently disabling authentication.
///
/// Call this *in addition to* [`validate_section`] when handling a
/// `PUT /config/gateway` request. Server startup and `check-config` do not
/// call this — operators with filesystem-level control of `aletheia.toml`
/// are trusted, and the loud [`warn_if_auth_disabled`] emission keeps the
/// posture visible. (#3383, #4240)
///
/// # Errors
///
/// Returns [`ValidationError`] if `auth.mode = "none"` and the env opt-in
/// is not set. Any other gateway section value (or absent `auth.mode`) is
/// accepted.
#[must_use]
#[expect(
    clippy::double_must_use,
    reason = "kanon lint requires explicit #[must_use] on pub fns returning Result"
)]
// kanon:ignore RUST/validate-returns-unit — returns Result<()> where Err carries the specific failure reason; Ok(()) signals validation passed
pub fn validate_auth_mode_policy(gateway_value: &Value) -> Result<(), ValidationError> {
    let Some(auth) = gateway_value.get("auth") else {
        return Ok(());
    };
    let Some(mode) = auth.get("mode").and_then(Value::as_str) else {
        return Ok(());
    };
    if mode == "none" && !auth_none_opt_in_enabled() {
        return ValidationSnafu {
            errors: vec![format!(
                "gateway.auth.mode = \"none\" disables all authentication and is \
                 rejected by default; set the environment variable {ALLOW_AUTH_NONE_ENV}=1 \
                 on the server process to opt in (intended for local dev only)"
            )],
        }
        .fail();
    }
    Ok(())
}

/// Return `true` when the operator has set `ALETHEIA_ALLOW_AUTH_NONE=1`.
///
/// This is the single gate that permits writing `auth_mode = "none"` through
/// the config API. Reading a TOML file at startup with `auth_mode = "none"` is
/// accepted regardless (operators have filesystem-level control of the file),
/// but a loud warning is emitted via [`warn_if_auth_disabled`].
#[must_use]
pub fn auth_none_opt_in_enabled() -> bool {
    std::env::var(ALLOW_AUTH_NONE_ENV).is_ok_and(|v| v == "1")
}

/// Emit a loud startup warning when authentication is disabled.
///
/// Called after the initial config load. Emits a single `warn!` event with the
/// prefix `SECURITY: auth disabled` so operators running with `auth_mode = "none"`
/// — even intentionally — see the consequence in every log aggregator. (#3383)
pub fn warn_if_auth_disabled(config: &AletheiaConfig) {
    if config.gateway.auth.mode == "none" {
        tracing::warn!(
            auth_mode = "none",
            "SECURITY: auth disabled — all endpoints are unauthenticated; \
             requests are served as role '{}'",
            config.gateway.auth.none_role,
        );
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
    if let Some(provider) = value.get("provider").and_then(Value::as_str) {
        if provider.is_empty() {
            errors.push("embedding.provider must not be empty".to_owned());
        } else if !matches!(provider, "mock" | "candle" | "openai-compat" | "voyage") {
            errors.push(format!(
                "embedding.provider must be one of: candle, openai-compat, voyage (mock is test-only); got '{provider}'"
            ));
        } else if provider == "openai-compat"
            && value
                .get("baseUrl")
                .and_then(Value::as_str)
                .is_none_or(|base_url| base_url.trim().is_empty())
        {
            errors.push(
                "embedding.baseUrl is required when embedding.provider = \"openai-compat\""
                    .to_owned(),
            );
        }
    }

    if let Some(base_url) = value.get("baseUrl").and_then(Value::as_str)
        && base_url.trim().is_empty()
    {
        errors.push("embedding.baseUrl must not be empty".to_owned());
    }

    if let Some(api_key_env) = value.get("apiKeyEnv").and_then(Value::as_str)
        && api_key_env.trim().is_empty()
    {
        errors.push("embedding.apiKeyEnv must not be empty".to_owned());
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

    if let Some(matrix) = value.get("matrix")
        && let Some(accounts) = matrix.get("accounts").and_then(Value::as_object)
    {
        for (account_id, account) in accounts {
            if account.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            if account
                .get("homeserver")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!(
                    "channels.matrix.accounts.{account_id}.homeserver must not be empty"
                ));
            }
            if account
                .get("accessTokenEnv")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!(
                    "channels.matrix.accounts.{account_id}.accessTokenEnv must not be empty"
                ));
            }
        }
    }
}

const KNOWN_CHANNEL_TYPES: &[&str] = &["signal", "matrix", "slack", "discord", "webhook", "http"];

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

fn validate_feature_flags(value: &Value, errors: &mut Vec<String>) {
    let Some(flags) = value.as_array() else {
        errors.push("feature_flags must be an array".to_owned());
        return;
    };

    let mut seen_keys = HashSet::new();
    for (i, flag) in flags.iter().enumerate() {
        let Some(flag) = flag.as_object() else {
            errors.push(format!("feature_flags[{i}] must be an object"));
            continue;
        };

        match flag.get("key").and_then(Value::as_str) {
            None | Some("") => errors.push(format!("feature_flags[{i}].key must not be empty")),
            Some(key) => {
                if !seen_keys.insert(key.to_owned()) {
                    errors.push(format!("feature_flags[{i}].key '{key}' is duplicated"));
                }
            }
        }

        if flag
            .get("description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            errors.push(format!("feature_flags[{i}].description must not be empty"));
        }

        if let Some(enabled) = flag.get("enabled")
            && !enabled.is_boolean()
        {
            errors.push(format!("feature_flags[{i}].enabled must be a boolean"));
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

fn validate_jwt(value: &Value, errors: &mut Vec<String>) {
    // WHY: 0 is valid (strict expiry on tightly synchronized hosts); a 300s
    // cap prevents misconfiguration that would hand out valid tokens long
    // past their intended lifetime, weakening revocation guarantees.
    const MAX_CLOCK_SKEW_LEEWAY_SECS: u64 = 300;
    if let Some(val) = value.get("clockSkewLeewaySecs").and_then(Value::as_u64)
        && val > MAX_CLOCK_SKEW_LEEWAY_SECS
    {
        errors.push(format!(
            "jwt.clockSkewLeewaySecs must not exceed {MAX_CLOCK_SKEW_LEEWAY_SECS} seconds"
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
        errors.push("retry.backoffMaxMs must be greater than or equal to backoffBaseMs".to_owned());
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
    check_range_u64(value, "maxSpawnedTasks", 1, 1_000, errors);
    check_range_u64(value, "gcIntervalSecs", 30, 3600, errors);
    check_range_u64(value, "loopDetectionWindow", 5, 500, errors);
    check_range_u64(value, "cycleDetectionMaxLen", 1, 100, errors);
    check_range_u64(value, "selfAuditEventThreshold", 1, 10_000, errors);
    check_range_u64(value, "managerHealthIntervalSecs", 5, 300, errors);
    check_range_u64(value, "managerPingTimeoutSecs", 1, 60, errors);
    check_range_u64(value, "managerMaxRestartBackoffSecs", 1, 86_400, errors);
    check_range_u64(value, "managerRestartDrainTimeoutSecs", 1, 600, errors);
    check_range_u64(value, "managerRestartDecayWindowSecs", 60, 86_400, errors);
    check_range_u64(value, "shutdownTimeoutSecs", 1, 3600, errors);
}

fn validate_knowledge(value: &Value, errors: &mut Vec<String>) {
    check_range_u64(value, "conflictMaxLlmCallsPerFact", 1, 20, errors);
    check_range_f64(value, "conflictIntraBatchDedupThreshold", 0.5, 1.0, errors);
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
    let min_len = value.get("extractionMinFactLength").and_then(Value::as_u64);
    let max_len = value.get("extractionMaxFactLength").and_then(Value::as_u64);
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
    check_range_f64(value, "concurrencyLatencyThresholdSecs", 1.0, 300.0, errors);
    check_range_u64(value, "complexityLowThreshold", 0, 100, errors);
    check_range_u64(value, "complexityHighThreshold", 0, 100, errors);

    // INVARIANT: low threshold must be <= high threshold.
    let low = value.get("complexityLowThreshold").and_then(Value::as_u64);
    let high = value.get("complexityHighThreshold").and_then(Value::as_u64);
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
    let default_limit = value.get("defaultHistoryLimit").and_then(Value::as_u64);
    let max_limit = value.get("maxHistoryLimit").and_then(Value::as_u64);
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
    if let Some(mode) = value.get("runnerOutputMode") {
        match mode.as_str() {
            Some("summary" | "brief" | "full") => {}
            Some(other) => errors.push(format!(
                "runnerOutputMode must be one of summary, brief, full (got {other})"
            )),
            None => errors.push("runnerOutputMode must be a string".to_owned()),
        }
    }

    // INVARIANT: backoff base must be <= backoff cap.
    let base = value.get("watchdogBackoffBaseSecs").and_then(Value::as_u64);
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

/// Validate configured external tool declarations.
const VALID_TOOL_KINDS: &[&str] = &["mcp", "http", "builtin"];
const VALID_HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];
const VALID_TOOL_GROUPS: &[&str] = &[
    "read",
    "edit",
    "command",
    "mcp",
    "spawn_subtask",
    "plan",
    "verify",
];
const VALID_REVERSIBILITIES: &[&str] = &[
    "fully_reversible",
    "reversible",
    "partially_reversible",
    "irreversible",
];
const VALID_REQUIRED_TOOL_FAILURE_MODES: &[&str] = &["fail_startup", "degraded"];

fn validate_tools(value: &Value, errors: &mut Vec<String>) {
    validate_required_tool_failure_mode(value, errors);
    validate_tool_group(value, "required", errors);
    validate_tool_group(value, "optional", errors);
}

fn validate_required_tool_failure_mode(value: &Value, errors: &mut Vec<String>) {
    let Some(mode) = value.get("requiredFailureMode") else {
        return;
    };
    let Some(mode) = mode.as_str() else {
        errors.push(format!(
            "tools.requiredFailureMode must be one of: {}",
            VALID_REQUIRED_TOOL_FAILURE_MODES.join(", ")
        ));
        return;
    };
    if !VALID_REQUIRED_TOOL_FAILURE_MODES.contains(&mode) {
        errors.push(format!(
            "tools.requiredFailureMode '{mode}' is invalid; must be one of: {}",
            VALID_REQUIRED_TOOL_FAILURE_MODES.join(", ")
        ));
    }
}

fn validate_tool_group(value: &Value, group: &str, errors: &mut Vec<String>) {
    let Some(entries) = value.get(group).and_then(Value::as_object) else {
        return;
    };

    for (name, entry) in entries {
        if ToolName::new(name).is_err() {
            errors.push(format!(
                "tools.{group}.{name} is not a valid tool name (alphanumeric, hyphens, underscores, 1-128 chars)"
            ));
            continue;
        }

        let Some(kind) = entry.get("type").and_then(Value::as_str) else {
            errors.push(format!("tools.{group}.{name}.type is required"));
            continue;
        };
        if !VALID_TOOL_KINDS.contains(&kind) {
            errors.push(format!(
                "tools.{group}.{name}.type '{kind}' is invalid; must be one of: mcp, http, builtin"
            ));
            continue;
        }

        if let Some(method) = entry.get("method").and_then(Value::as_str)
            && !VALID_HTTP_METHODS.contains(&method)
        {
            errors.push(format!(
                "tools.{group}.{name}.method '{method}' is invalid; must be one of: get, post, put, delete, patch"
            ));
        }

        match kind {
            "http" => {
                let Some(endpoint) = entry.get("endpoint").and_then(Value::as_str) else {
                    errors.push(format!(
                        "tools.{group}.{name}.endpoint is required for type 'http'"
                    ));
                    continue;
                };
                if endpoint.is_empty() {
                    errors.push(format!("tools.{group}.{name}.endpoint must not be empty"));
                // kanon:ignore SECURITY/insecure-transport — scheme prefix literals used for validation, not as connection URLs
                } else if !(endpoint.starts_with("http://") || endpoint.starts_with("https://")) {
                    errors.push(format!(
                        "tools.{group}.{name}.endpoint must use http:// or https://"
                    ));
                }
                validate_http_tool_policy(entry, group, name, errors);
            }
            "mcp" => {
                let has_endpoint = entry
                    .get("endpoint")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty());
                let has_command = entry
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty());
                if !has_endpoint && !has_command {
                    errors.push(format!(
                        "tools.{group}.{name}: mcp tools require either endpoint or command"
                    ));
                }
                // kanon:ignore SECURITY/insecure-transport — scheme prefix literals used for validation, not as connection URLs
                if has_endpoint
                    && let Some(endpoint) = entry.get("endpoint").and_then(Value::as_str)
                    // kanon:ignore SECURITY/insecure-transport — string literal for scheme validation, not an actual endpoint
                    && !(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
                {
                    errors.push(format!(
                        "tools.{group}.{name}.endpoint must use http:// or https://"
                    ));
                }
                if let Some(auth) = entry.get("auth") {
                    validate_tool_auth(&format!("tools.{group}.{name}.auth"), auth, errors);
                }
            }
            _ => {} // other tool types carry no additional endpoint constraints
        }
    }
}

fn validate_http_tool_policy(entry: &Value, group: &str, name: &str, errors: &mut Vec<String>) {
    validate_http_tool_groups(entry, group, name, errors);
    validate_http_tool_reversibility(entry, group, name, errors);
}

fn validate_http_tool_groups(entry: &Value, group: &str, name: &str, errors: &mut Vec<String>) {
    let path = format!("tools.{group}.{name}.groups");
    let groups_value = entry.get("groups").filter(|value| !value.is_null());
    let Some(groups) = groups_value else {
        if group == "required" {
            errors.push(format!(
                "{path} is required for required HTTP tools; use at least one of: {}",
                VALID_TOOL_GROUPS.join(", ")
            ));
        }
        return;
    };

    let Some(groups) = groups.as_array() else {
        errors.push(format!("{path} must be a non-empty list of tool groups"));
        return;
    };
    if groups.is_empty() {
        errors.push(format!("{path} must not be empty"));
        return;
    }

    for value in groups {
        let Some(group_name) = value.as_str() else {
            errors.push(format!("{path} entries must be strings"));
            continue;
        };
        if !VALID_TOOL_GROUPS.contains(&group_name) {
            errors.push(format!(
                "{path} contains invalid group '{group_name}'; must be one of: {}",
                VALID_TOOL_GROUPS.join(", ")
            ));
        }
    }
}

fn validate_http_tool_reversibility(
    entry: &Value,
    group: &str,
    name: &str,
    errors: &mut Vec<String>,
) {
    let path = format!("tools.{group}.{name}.reversibility");
    let reversibility = entry.get("reversibility").filter(|value| !value.is_null());
    let Some(reversibility) = reversibility else {
        if group == "required" {
            errors.push(format!(
                "{path} is required for required HTTP tools; use one of: {}",
                VALID_REVERSIBILITIES.join(", ")
            ));
        }
        return;
    };

    let Some(reversibility) = reversibility.as_str() else {
        errors.push(format!("{path} must be a reversibility string"));
        return;
    };
    if !VALID_REVERSIBILITIES.contains(&reversibility) {
        errors.push(format!(
            "{path} '{reversibility}' is invalid; must be one of: {}",
            VALID_REVERSIBILITIES.join(", ")
        ));
    }
}

/// Validate an `auth` block inside a tool declaration.
///
/// Rejects empty tokens, header names/values, or env-var references so
/// auth-required configs fail fast at validation time (#4633).
fn validate_tool_auth(path: &str, value: &Value, errors: &mut Vec<String>) {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        errors.push(format!("{path}.type is required"));
        return;
    };

    match kind {
        "bearer" => {
            if value
                .get("token")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!("{path}.token is required and must not be empty"));
            }
        }
        "header" => {
            if value
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!("{path}.name is required and must not be empty"));
            }
            if value
                .get("value")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!("{path}.value is required and must not be empty"));
            }
        }
        "env_token" => {
            if value
                .get("header_name")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!(
                    "{path}.header_name is required and must not be empty"
                ));
            }
            if value
                .get("env_var")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                errors.push(format!("{path}.env_var is required and must not be empty"));
            }
        }
        other => errors.push(format!(
            "{path}.type '{other}' is invalid; must be one of: bearer, header, env_token"
        )),
    }
}

/// Validate the [`LlmProviderConfig`](crate::config::LlmProviderConfig) list
/// (#3424, #3414).
///
/// Rejects entries with empty names, unknown provider kinds, OpenAI-compatible
/// entries missing a base URL, malformed subprocess fields, or duplicate
/// provider names.
fn validate_providers(value: &Value, errors: &mut Vec<String>) {
    let Some(entries) = value.as_array() else {
        // WHY: missing or empty provider list is a valid config — the
        // legacy single-Anthropic path remains the default.
        return;
    };

    let mut seen_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        match entry.get("name").and_then(Value::as_str) {
            None | Some("") => {
                errors.push(format!("providers[{i}].name must not be empty"));
            }
            Some(name) => {
                if !seen_names.insert(name) {
                    errors.push(format!(
                        "providers[{i}].name '{name}' is duplicated (names must be unique)"
                    ));
                }
            }
        }

        let provider_kind = entry.get("providerType").and_then(Value::as_str);
        match provider_kind {
            None | Some("") => {
                errors.push(format!(
                    "providers[{i}].providerType must be one of: anthropic, openai, open-ai-compatible, openai-compatible, claude-code, codex_oauth, codex-oauth"
                ));
            }
            Some(kind) => {
                if !matches!(
                    kind,
                    "anthropic"
                        | "openai"
                        | "open-ai-compatible"
                        | "openai-compatible"
                        | "claude-code"
                        | "codex_oauth"
                        | "codex-oauth"
                ) {
                    errors.push(format!(
                        "providers[{i}].providerType '{kind}' is not recognized (expected one of: anthropic, openai, open-ai-compatible, openai-compatible, claude-code, codex_oauth, codex-oauth)"
                    ));
                }
                if matches!(kind, "open-ai-compatible" | "openai-compatible")
                    && entry
                        .get("baseUrl")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                {
                    errors.push(format!(
                        "providers[{i}].baseUrl is required for providerType = openai-compatible"
                    ));
                }
                validate_provider_kind_specific_fields(i, entry, kind, errors);
            }
        }
        validate_provider_models(i, entry, errors);
        if let Some(target) = entry.get("deploymentTarget").and_then(Value::as_str)
            && !matches!(
                target,
                "cloud" | "localhosted" | "local_hosted" | "local-hosted" | "embedded"
            )
        {
            errors.push(format!(
                "providers[{i}].deploymentTarget '{target}' is not recognized (expected one of: cloud, localhosted, local_hosted, local-hosted, embedded)"
            ));
        }
        if let Some(api_family) = entry.get("apiFamily").and_then(Value::as_str)
            && !matches!(api_family, "chat-completions" | "responses")
        {
            errors.push(format!(
                "providers[{i}].apiFamily '{api_family}' is not recognized (expected one of: chat-completions, responses)"
            ));
        }
    }
}

fn validate_provider_kind_specific_fields(
    i: usize,
    entry: &Value,
    kind: &str,
    errors: &mut Vec<String>,
) {
    let subprocess = matches!(kind, "claude-code" | "codex_oauth" | "codex-oauth");
    if subprocess {
        for field in ["baseUrl", "apiKeyEnv", "apiFamily"] {
            if entry.get(field).is_some() {
                errors.push(format!(
                    "providers[{i}].{field} is not valid for subprocess providerType = {kind}; remove it or use an HTTP provider type"
                ));
            }
        }
        validate_optional_non_empty_string(i, entry, "binary", errors);
        validate_optional_non_empty_string(i, entry, "workdir", errors);
        validate_provider_timeout(i, entry, errors);
        return;
    }

    for field in ["binary", "workdir", "timeoutSecs"] {
        if entry.get(field).is_some() {
            errors.push(format!(
                "providers[{i}].{field} is only valid for providerType = claude-code or codex-oauth"
            ));
        }
    }
}

fn validate_optional_non_empty_string(
    i: usize,
    entry: &Value,
    field: &str,
    errors: &mut Vec<String>,
) {
    let Some(value) = entry.get(field) else {
        return;
    };
    match value.as_str() {
        Some(text) if !text.is_empty() => {}
        _ => errors.push(format!("providers[{i}].{field} must be a non-empty string")),
    }
}

fn validate_provider_timeout(i: usize, entry: &Value, errors: &mut Vec<String>) {
    let Some(value) = entry.get("timeoutSecs") else {
        return;
    };
    let Some(timeout_secs) = value.as_u64() else {
        errors.push(format!("providers[{i}].timeoutSecs must be an integer"));
        return;
    };
    if !(5..=3600).contains(&timeout_secs) {
        errors.push(format!(
            "providers[{i}].timeoutSecs must be between 5 and 3600 seconds, got {timeout_secs}"
        ));
    }
}

fn validate_provider_models(i: usize, entry: &Value, errors: &mut Vec<String>) {
    let Some(models) = entry.get("models") else {
        return;
    };
    let Some(models) = models.as_array() else {
        errors.push(format!("providers[{i}].models must be an array of strings"));
        return;
    };
    for (model_i, model) in models.iter().enumerate() {
        match model.as_str() {
            Some(text) if !text.is_empty() => {}
            _ => errors.push(format!(
                "providers[{i}].models[{model_i}] must be a non-empty string"
            )),
        }
    }
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod validate_tests;

#[cfg(test)]
mod validate_openai_api_family_tests;
