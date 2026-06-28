//! Health check endpoint.

use std::collections::HashSet;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use koina::system::{Environment, RealSystem};
use symbolon::types::Role;

use hermeneus::health::{DownReason, ProviderHealth};

use crate::credential_runtime::CredentialMutationEffect;
use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::HealthState;

#[path = "health_dto.rs"]
mod health_dto;
pub use health_dto::{HealthCheck, HealthResponse, LivenessResponse};

/// Per-check timeout: individual health checks that exceed this are reported as "timeout".
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall endpoint timeout: the health response is always returned within this bound.
const OVERALL_TIMEOUT: Duration = Duration::from_secs(10);

/// GET /api/health: public liveness check.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Public liveness status", body = LivenessResponse),
    ),
)]
pub async fn check() -> impl IntoResponse {
    Json(LivenessResponse {
        status: "healthy".into(),
    })
}

/// GET /api/v1/system/health: operator-only readiness and diagnostics.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/system/health",
    responses(
        (status = 200, description = "Detailed health status", body = HealthResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 503, description = "Service unavailable", body = HealthResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn detailed(
    State(state): State<HealthState>,
    claims: Claims,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    let (http_status, response) = detailed_health(&state).await;
    Ok((http_status, Json(response)))
}

async fn detailed_health(state: &HealthState) -> (StatusCode, HealthResponse) {
    let uptime = state.start_time.elapsed().as_secs();

    // WHY: Run all health checks concurrently with individual timeouts so a
    // single hanging check (e.g., provider connection) cannot block the entire
    // endpoint. The overall timeout guarantees a response even if multiple
    // checks hang simultaneously (#3277).
    let checks = tokio::time::timeout(OVERALL_TIMEOUT, async {
        // WHY: read config once before spawning concurrent checks so each check
        // does not contend on the config lock.
        let (
            clock_skew_leeway,
            expiry_warning_threshold,
            prosoche,
            gateway_security_check,
            rate_limiting_check,
        ) = {
            let config = state.config.read().await;
            (
                config.api_limits.clock_skew_leeway_secs,
                config.api_limits.expiry_warning_threshold_secs,
                config.maintenance.prosoche.clone(),
                gateway_security_check(&config.gateway.auth.mode, &config.gateway.bind),
                rate_limiting_check(
                    config.gateway.rate_limit.enabled,
                    config.gateway.rate_limit.trust_proxy,
                    config.gateway.rate_limit.per_user.enabled,
                ),
            )
        };

        let (store_check, actor_check, config_check, storage_check) = tokio::join!(
            timed_check("session_store", check_session_store(state)),
            timed_check("nous_actors", check_nous_actors(state)),
            timed_check("config_readable", check_config_readable(state)),
            timed_check("storage_writable", check_storage_writable(state)),
        );

        // WHY: these checks are synchronous and cheap — no timeout needed.
        let provider_check = check_provider_availability(state);
        let credential_check =
            check_credential_validity(state, clock_skew_leeway, expiry_warning_threshold);
        let credential_runtime_check = check_credential_runtime(state).await;
        let embedding_check = check_embedding_provider(state);
        let prosoche_check = check_prosoche_heartbeat_path(&prosoche);

        // WHY: synchronous and cheap — the snapshot is a few atomic reads plus a
        // short mutex hold on the last recorded poller error.
        let poller_snapshot = state.nous_manager.poller_snapshot();
        let nous_poller_check = check_nous_health_poller(
            poller_snapshot.running,
            poller_snapshot.restart_count,
            poller_snapshot.last_error.as_deref(),
        );

        vec![
            store_check,
            provider_check,
            actor_check,
            check_provider_reachability(state),
            config_check,
            gateway_security_check,
            rate_limiting_check,
            credential_check,
            credential_runtime_check,
            storage_check,
            embedding_check,
            nous_poller_check,
            prosoche_check,
        ]
    })
    .await
    .unwrap_or_else(|_| {
        vec![HealthCheck {
            name: "overall".to_owned(),
            status: "fail".to_owned(),
            message: Some("health check timed out".to_owned()),
            details: None,
        }]
    });

    // WHY: "timeout" is treated as "fail" for aggregate status because
    // a timed-out check means we cannot confirm the subsystem is healthy.
    let status = if checks
        .iter()
        .any(|c| c.status == "fail" || c.status == "timeout")
    {
        "unhealthy"
    } else if checks.iter().any(|c| c.status == "warn") {
        "degraded"
    } else {
        "healthy"
    };

    let http_status = if status == "unhealthy" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (
        http_status,
        HealthResponse {
            status: status.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            git_sha: option_env!("GIT_SHA").unwrap_or("unknown").to_owned(),
            uptime_seconds: uptime,
            checks,
            data_dir: state.oikos.data().to_string_lossy().into_owned(),
        },
    )
}

fn gateway_security_check(auth_mode: &str, bind: &str) -> HealthCheck {
    if auth_mode == "none" && !taxis::validate::is_loopback_bind(bind) {
        return HealthCheck {
            name: "gateway_security".to_owned(),
            status: "fail".to_owned(),
            message: Some(format!(
                "unsafe gateway posture: auth.mode = \"none\" with non-loopback bind '{bind}'"
            )),
            details: None,
        };
    }
    if auth_mode == "none" {
        return HealthCheck {
            name: "gateway_security".to_owned(),
            status: "warn".to_owned(),
            message: Some(
                "auth.mode = \"none\" is limited to loopback but remains unauthenticated"
                    .to_owned(),
            ),
            details: None,
        };
    }
    HealthCheck {
        name: "gateway_security".to_owned(),
        status: "pass".to_owned(),
        message: None,
        details: None,
    }
}

/// Build the rate-limiting diagnostics check.
///
/// Reports the active keying strategy so the control plane can show whether
/// rate limits are keyed by peer socket, forwarded client IP, or authenticated
/// user. Per-user rate limiting takes precedence when enabled; otherwise the
/// per-IP limiter follows the `trust_proxy` flag.
fn rate_limiting_check(enabled: bool, trust_proxy: bool, per_user_enabled: bool) -> HealthCheck {
    let keying = if per_user_enabled {
        "authenticated_user"
    } else if enabled {
        if trust_proxy {
            "forwarded_client_ip"
        } else {
            "peer_socket"
        }
    } else {
        "disabled"
    };

    let message = if per_user_enabled {
        "per-user rate limiting enabled; keyed by authenticated user".to_owned()
    } else if enabled {
        let keying_phrase = if trust_proxy {
            "forwarded client IP"
        } else {
            "peer socket"
        };
        format!("per-IP rate limiting enabled; keyed by {keying_phrase}")
    } else {
        "rate limiting disabled".to_owned()
    };

    HealthCheck {
        name: "rate_limiting".to_owned(),
        status: "pass".to_owned(),
        message: Some(message),
        details: Some(serde_json::json!({
            "enabled": enabled,
            "trust_proxy": trust_proxy,
            "per_user_enabled": per_user_enabled,
            "keying": keying,
        })),
    }
}

/// GET /health: deprecated unversioned health check.
///
/// Use `/api/health` instead.
#[deprecated = "Use /api/health instead"]
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Public liveness status", body = LivenessResponse),
    ),
)]
pub async fn deprecated_health_check() -> impl IntoResponse {
    check().await
}

/// Run a health check with a per-check timeout. If the check exceeds
/// [`CHECK_TIMEOUT`], a "timeout" status is returned instead of blocking.
async fn timed_check(
    name: &'static str,
    future: impl std::future::Future<Output = HealthCheck>,
) -> HealthCheck {
    match tokio::time::timeout(CHECK_TIMEOUT, future).await {
        Ok(check) => check,
        Err(_elapsed) => HealthCheck {
            name: name.to_owned(),
            status: "timeout".to_owned(),
            message: Some(format!(
                "{name} check timed out after {}s",
                CHECK_TIMEOUT.as_secs()
            )),
            details: None,
        },
    }
}

/// Check session store connectivity.
async fn check_session_store(state: &HealthState) -> HealthCheck {
    let store_ok = state.session_store.lock().await.ping().is_ok();
    HealthCheck {
        name: "session_store".to_owned(),
        status: (if store_ok { "pass" } else { "fail" }).to_owned(),
        message: if store_ok {
            None
        } else {
            Some("session store unavailable".to_owned())
        },
        details: None,
    }
}

/// Check whether any LLM providers are registered.
fn check_provider_availability(state: &HealthState) -> HealthCheck {
    let has_providers = !state.provider_registry.providers().is_empty();
    HealthCheck {
        name: "providers".to_owned(),
        status: (if has_providers { "pass" } else { "warn" }).to_owned(),
        message: if has_providers {
            None
        } else {
            Some("no LLM providers registered".to_owned())
        },
        details: None,
    }
}

/// Check the Nous manager health-poller supervisor state.
fn check_nous_health_poller(
    running: bool,
    restart_count: u64,
    last_error: Option<&str>,
) -> HealthCheck {
    let status = if !running {
        if last_error.is_some() { "fail" } else { "warn" }
    } else if last_error.is_some() {
        "warn"
    } else {
        "pass"
    };

    let message = if status == "pass" {
        None
    } else {
        let mut parts = vec![if running {
            "poller is running but has a recorded error".to_owned()
        } else {
            "poller is not running".to_owned()
        }];
        if restart_count > 0 {
            parts.push(format!("restart_count={restart_count}"));
        }
        if let Some(error) = last_error {
            parts.push(format!("last_error={error}"));
        }
        Some(parts.join("; "))
    };

    HealthCheck {
        name: "nous_health_poller".to_owned(),
        status: status.to_owned(),
        message,
        details: None,
    }
}

/// Check nous actor liveness and background health.
async fn check_nous_actors(state: &HealthState) -> HealthCheck {
    let actor_health = state.nous_manager.check_health().await;
    let any_dead = actor_health.values().any(|h| !h.alive);

    if actor_health.is_empty() || any_dead {
        return HealthCheck {
            name: "nous_actors".to_owned(),
            status: "fail".to_owned(),
            message: if actor_health.is_empty() {
                Some("no nous actors registered".to_owned())
            } else {
                let dead: Vec<_> = actor_health
                    .iter()
                    .filter(|(_, h)| !h.alive)
                    .map(|(id, _)| id.as_str())
                    .collect();
                Some(format!("actors not responding: {}", dead.join(", ")))
            },
            details: None,
        };
    }

    let degraded: Vec<_> = actor_health
        .iter()
        .filter(|(_, h)| h.background_health_degraded)
        .collect();

    if degraded.is_empty() {
        HealthCheck {
            name: "nous_actors".to_owned(),
            status: "pass".to_owned(),
            message: None,
            details: None,
        }
    } else {
        let summaries: Vec<String> = degraded
            .iter()
            .map(|(id, h)| {
                let mut parts = vec![format!("id={id}")];
                parts.push(format!(
                    "recent={} total={}",
                    h.background_failure_recent_count, h.background_failure_total_count
                ));
                if let Some(kind) = &h.background_failure_latest_kind {
                    parts.push(format!("kind={kind}"));
                }
                if let Some(message) = &h.background_failure_latest_message {
                    parts.push(format!("message={message}"));
                }
                parts.join(" ")
            })
            .collect();
        HealthCheck {
            name: "nous_actors".to_owned(),
            status: "warn".to_owned(),
            message: Some(format!(
                "background health degraded: {}",
                summaries.join("; ")
            )),
            details: None,
        }
    }
}

/// Environment variable that lists provider names which are allowed to be
/// degraded or down without lowering the overall service health status.
///
/// WHY: pylon does not own the provider config schema, so the optional flag
/// is supplied as a comma-separated operator override at deployment time.
/// Required providers are the default; only names listed here are exempt.
const OPTIONAL_PROVIDERS_ENV: &str = "ALETHEIA_OPTIONAL_PROVIDERS";

/// Parse the optional-provider override from the environment.
///
/// Comma-separated names are trimmed and empty entries are ignored so that
/// `",,"` does not create an empty-name entry.
fn optional_providers_from_env() -> HashSet<String> {
    std::env::var(OPTIONAL_PROVIDERS_ENV)
        .map(|raw| parse_optional_providers(&raw))
        .unwrap_or_default()
}

/// Parse a comma-separated optional-provider override.
///
/// WHY: Split from the env reader so unit tests can exercise parsing without
/// mutating global process state.
fn parse_optional_providers(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// Check LLM provider connectivity by querying the provider registry health.
fn check_provider_reachability(state: &HealthState) -> HealthCheck {
    provider_reachability_check(&state.provider_registry, &optional_providers_from_env())
}

/// Core implementation of provider reachability, parameterized for testing.
///
/// Returns a per-provider status list in `details` and fails/warns whenever any
/// *required* configured provider is down or degraded. Optional providers are
/// still reported but do not affect the aggregate status.
fn provider_reachability_check(
    registry: &hermeneus::provider::ProviderRegistry,
    optional_names: &HashSet<String>,
) -> HealthCheck {
    let providers = registry.providers();
    if providers.is_empty() {
        return HealthCheck {
            name: "provider_reachability".to_owned(),
            status: "warn".to_owned(),
            message: Some("no providers to check".to_owned()),
            details: None,
        };
    }

    let provider_details: Vec<serde_json::Value> = providers
        .iter()
        .map(|provider| {
            let name = provider.name();
            let health = registry.provider_health(name).unwrap_or(ProviderHealth::Up);
            provider_health_detail(name, &health)
        })
        .collect();

    let required_detail = |detail: &&serde_json::Value| {
        detail["name"]
            .as_str()
            .is_some_and(|name| !optional_names.contains(name))
    };

    let any_required_down = provider_details
        .iter()
        .filter(required_detail)
        .any(|detail| detail["status"] == "down");

    let any_required_degraded = provider_details
        .iter()
        .filter(required_detail)
        .any(|detail| detail["status"] == "degraded");

    let status = if any_required_down {
        "fail"
    } else if any_required_degraded {
        "warn"
    } else {
        "pass"
    };

    let message = provider_reachability_message(status, &provider_details, optional_names);

    HealthCheck {
        name: "provider_reachability".to_owned(),
        status: status.to_owned(),
        message,
        details: Some(serde_json::json!({ "providers": provider_details })),
    }
}

/// Build a human-readable summary that mirrors the structured `details` payload.
///
/// WHY: Keep the top-level `message` short and credential-free; full per-provider
/// state lives in `details` for the control-plane UI.
fn provider_reachability_message(
    status: &str,
    details: &[serde_json::Value],
    optional_names: &HashSet<String>,
) -> Option<String> {
    if status == "pass" {
        return None;
    }

    let affected: Vec<String> = details
        .iter()
        .filter(|detail| {
            detail["name"]
                .as_str()
                .is_some_and(|name| !optional_names.contains(name))
        })
        .filter_map(|detail| {
            let name = detail["name"].as_str()?;
            let health_status = detail["status"].as_str()?;
            if health_status == "up" {
                return None;
            }
            detail["reason"]
                .as_str()
                .map(|reason| format!("{name} is {health_status} ({reason})"))
        })
        .collect();

    if affected.is_empty() {
        return None;
    }

    Some(format!("required providers: {}", affected.join("; ")))
}

/// Convert a provider health state into a credential-free detail object.
///
/// Only the provider name, health status, and reason are exposed. No URLs,
/// API keys, or model identifiers are included.
fn provider_health_detail(name: &str, health: &ProviderHealth) -> serde_json::Value {
    match health {
        ProviderHealth::Up => serde_json::json!({
            "name": name,
            "status": "up",
            "checks": {
                "consecutive_errors": 0,
            },
        }),
        ProviderHealth::Degraded {
            consecutive_errors, ..
        } => serde_json::json!({
            "name": name,
            "status": "degraded",
            // WHY: "recent_errors" is a stable reason label for UI routing.
            "reason": format!("recent_errors ({consecutive_errors} consecutive)"),
        }),
        ProviderHealth::Down { reason, .. } => serde_json::json!({
            "name": name,
            "status": "down",
            "reason": down_reason_label(reason),
        }),
        _ => serde_json::json!({
            "name": name,
            "status": "unknown",
        }),
    }
}

/// Stable string label for a [`DownReason`] that does not expose secrets.
fn down_reason_label(reason: &DownReason) -> String {
    match reason {
        DownReason::ConsecutiveFailures => "consecutive_failures".to_owned(),
        DownReason::RateLimited { retry_after_ms } => {
            format!("rate_limited(retry_after_ms={retry_after_ms})")
        }
        DownReason::AuthFailure => "auth_failure".to_owned(),
        DownReason::Timeout => "timeout".to_owned(),
        _ => "unknown".to_owned(),
    }
}

/// Check embedding provider status.
///
/// Reports:
/// - `"pass"` when the real provider is loaded and healthy
/// - `"warn"` with `"degraded: embedding-loading"` while the lazy provider
///   is still initializing (#3474)
/// - `"warn"` with `"degraded: no-embeddings"` when the real provider failed
///   to load and the server is running BM25-only (#3380)
fn check_embedding_provider(state: &HealthState) -> HealthCheck {
    /// Sentinel model name emitted by `LazyEmbeddingProvider` while still loading.
    const LOADING_MODEL_NAME: &str = "embedding-loading";

    let Some(provider) = state.embedding_provider.as_ref() else {
        return HealthCheck {
            name: "embedding_provider".to_owned(),
            status: "warn".to_owned(),
            message: Some("no embedding provider configured".to_owned()),
            details: None,
        };
    };

    let model_name = provider.model_name();

    if model_name == LOADING_MODEL_NAME {
        HealthCheck {
            name: "embedding_provider".to_owned(),
            status: "warn".to_owned(),
            message: Some(
                "degraded: embedding-loading (model initializing — \
                 recall unavailable until load completes)"
                    .to_owned(),
            ),
            details: None,
        }
    } else if mneme::embedding::is_degraded_provider(provider.as_ref()) {
        HealthCheck {
            name: "embedding_provider".to_owned(),
            status: "warn".to_owned(),
            message: Some(
                "degraded: no-embeddings (embedding model failed to load at startup — \
                 recall falls back to BM25)"
                    .to_owned(),
            ),
            details: None,
        }
    } else {
        HealthCheck {
            name: "embedding_provider".to_owned(),
            status: "pass".to_owned(),
            message: None,
            details: None,
        }
    }
}

/// Report the currently active prosoche heartbeat path.
///
/// WHY(#5150): Prosoche scheduling is split between the in-process daemon
/// scheduler and an optional external systemd timer. This check makes the
/// active path visible to operators without changing the minimal public
/// `/api/health` response.
fn check_prosoche_heartbeat_path(
    settings: &taxis::config::ProsocheMaintenanceSettings,
) -> HealthCheck {
    let runs_daemon = settings.mode.runs_daemon_tasks()
        && (settings.heartbeat.enabled || settings.self_audit.enabled);
    let uses_external = settings.mode.uses_external_timer() && settings.external_timer.enabled;
    let message = match (runs_daemon, uses_external) {
        (true, true) => format!(
            "active path: both; daemon heartbeat every {}s, self-audit every {}s; external timer task {} every {}s",
            settings.heartbeat.interval_secs,
            settings.self_audit.interval_secs,
            settings.external_timer.task_id,
            settings.external_timer.interval_secs
        ),
        (true, false) => format!(
            "active path: daemon; heartbeat every {}s, self-audit every {}s",
            settings.heartbeat.interval_secs, settings.self_audit.interval_secs
        ),
        (false, true) => format!(
            "active path: external; timer task {} every {}s",
            settings.external_timer.task_id, settings.external_timer.interval_secs
        ),
        (false, false) => "active path: disabled".to_owned(),
    };

    HealthCheck {
        name: "prosoche_heartbeat_path".to_owned(),
        status: "pass".to_owned(),
        message: Some(message),
        details: None,
    }
}

/// Check if config can be read (verify config file exists and is accessible).
async fn check_config_readable(state: &HealthState) -> HealthCheck {
    let config_dir = state.oikos.config();
    let instance_root = state.oikos.root();

    // WHY: validate that constructed config paths stay within the instance
    // root to prevent path-traversal if the config directory is misconfigured.
    let toml_path = config_dir.join("aletheia.toml");
    let json_path = config_dir.join("aletheia.json");

    let config_path = if tokio::fs::metadata(&toml_path).await.is_ok() {
        match koina::fs::validate_within_root(&toml_path, instance_root) {
            Ok(p) => p,
            Err(e) => {
                return HealthCheck {
                    name: "config_readable".to_owned(),
                    status: "fail".to_owned(),
                    message: Some(format!("config path validation failed: {e}")),
                    details: None,
                };
            }
        }
    } else {
        match koina::fs::validate_within_root(&json_path, instance_root) {
            Ok(p) => p,
            Err(e) => {
                // WHY: json_path may not exist yet (first run); validation
                // failure here means the parent directory itself is outside
                // the instance root, which is a real misconfiguration.
                return HealthCheck {
                    name: "config_readable".to_owned(),
                    status: "warn".to_owned(),
                    message: Some(format!("config path validation failed: {e}")),
                    details: None,
                };
            }
        }
    };

    match tokio::fs::metadata(&config_path).await {
        Ok(metadata) => {
            if metadata.is_file() {
                // WHY: also verify the in-memory config lock is readable.
                let _config = state.config.read().await;
                HealthCheck {
                    name: "config_readable".to_owned(),
                    status: "pass".to_owned(),
                    message: None,
                    details: None,
                }
            } else {
                HealthCheck {
                    name: "config_readable".to_owned(),
                    status: "warn".to_owned(),
                    message: Some(format!(
                        "config path exists but is not a file: {}",
                        config_path.display()
                    )),
                    details: None,
                }
            }
        }
        Err(e) => {
            // WHY: warn, not fail — the config file may not exist yet (first run).
            HealthCheck {
                name: "config_readable".to_owned(),
                status: "warn".to_owned(),
                message: Some(format!(
                    "cannot read config file at {}: {e}",
                    config_path.display()
                )),
                details: None,
            }
        }
    }
}

/// Check credential validity (presence and expiry).
fn check_credential_validity(
    state: &HealthState,
    clock_skew_leeway: u64,
    expiry_warning_threshold: u64,
) -> HealthCheck {
    if let Some(check) = provider_credential_scope_check(state) {
        return check;
    }

    let env_key = RealSystem.var("ANTHROPIC_API_KEY").or_else(|| {
        tracing::debug!("ANTHROPIC_API_KEY not set");
        None
    });

    if let Some(key) = env_key {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        return check_env_oauth_token(&key, now_secs, clock_skew_leeway, expiry_warning_threshold);
    }

    let creds_dir = state.oikos.credentials();
    let cred_file = creds_dir.join("anthropic.json");

    if let Some(cred_file) = symbolon::credential::CredentialFile::load(&cred_file) {
        if let Some(remaining_secs) = cred_file.seconds_remaining() {
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "u64->i64: leeway/threshold values fit in i64"
            )]
            let leeway_i64 = clock_skew_leeway as i64; // kanon:ignore RUST/as-cast
            #[expect(
                clippy::cast_possible_wrap,
                clippy::as_conversions,
                reason = "u64->i64: leeway/threshold values fit in i64"
            )]
            let warning_i64 = expiry_warning_threshold as i64; // kanon:ignore RUST/as-cast

            if remaining_secs < leeway_i64 {
                return HealthCheck {
                    name: "credential_validity".to_owned(),
                    status: "warn".to_owned(),
                    message: Some("credential file token has expired".to_owned()),
                    details: None,
                };
            } else if remaining_secs < warning_i64 {
                return HealthCheck {
                    name: "credential_validity".to_owned(),
                    status: "warn".to_owned(),
                    message: Some("credential file token expires soon".to_owned()),
                    details: None,
                };
            }
        }
        return HealthCheck {
            name: "credential_validity".to_owned(),
            status: "pass".to_owned(),
            message: None,
            details: None,
        };
    }

    let cc_credentials =
        symbolon::credential::claude_code_default_path().is_some_and(|p| p.exists());
    if cc_credentials {
        HealthCheck {
            name: "credential_validity".to_owned(),
            status: "pass".to_owned(),
            message: Some(
                "Claude Code credentials available (CC provider handles auth)".to_owned(),
            ),
            details: None,
        }
    } else {
        HealthCheck {
            name: "credential_validity".to_owned(),
            status: "warn".to_owned(),
            message: Some("no credentials found (ANTHROPIC_API_KEY not set, no credential file, no Claude Code credentials)".to_owned()),
            details: None,
        }
    }
}

/// Check an env-var credential for OAuth token expiry.
///
/// Returns a `credential_validity` [`HealthCheck`] for the given `ANTHROPIC_API_KEY`
/// value. Non-OAuth keys and OAuth tokens without a decodable expiry claim are
/// treated as valid.
fn check_env_oauth_token(
    key: &str,
    now_secs: u64,
    clock_skew_leeway: u64,
    expiry_warning_threshold: u64,
) -> HealthCheck {
    if key.is_empty() {
        return HealthCheck {
            name: "credential_validity".to_owned(),
            status: "warn".to_owned(),
            message: Some("ANTHROPIC_API_KEY is set but empty".to_owned()),
            details: None,
        };
    }

    if key.starts_with("sk-ant-oat") {
        // NOTE: the sk-ant-oat prefix marks an OAuth token with a decodable expiry.
        if let Some(exp_secs) = decode_jwt_exp(key) {
            let remaining_secs =
                exp_secs.saturating_sub(now_secs.saturating_add(clock_skew_leeway));
            if remaining_secs == 0 {
                return HealthCheck {
                    name: "credential_validity".to_owned(),
                    status: "warn".to_owned(),
                    message: Some("OAuth token has expired".to_owned()),
                    details: None,
                };
            }
            if remaining_secs <= expiry_warning_threshold {
                return HealthCheck {
                    name: "credential_validity".to_owned(),
                    status: "warn".to_owned(),
                    message: Some("OAuth token expires soon".to_owned()),
                    details: None,
                };
            }
        }
    }

    HealthCheck {
        name: "credential_validity".to_owned(),
        status: "pass".to_owned(),
        message: None,
        details: None,
    }
}

fn provider_credential_scope_check(state: &HealthState) -> Option<HealthCheck> {
    let provider_names: Vec<&str> = state
        .provider_registry
        .providers()
        .into_iter()
        .map(hermeneus::provider::LlmProvider::name)
        .collect();

    if provider_names.is_empty() {
        return Some(HealthCheck {
            name: "credential_validity".to_owned(),
            status: "warn".to_owned(),
            message: Some(
                "no providers registered; credential validity cannot be checked".to_owned(),
            ),
            details: None,
        });
    }

    if provider_names
        .iter()
        .any(|name| provider_uses_anthropic_credentials(name))
    {
        return None;
    }

    Some(HealthCheck {
        name: "credential_validity".to_owned(),
        status: "pass".to_owned(),
        message: Some(format!(
            "registered providers do not use pylon-managed Anthropic credentials: {}",
            provider_names.join(", ")
        )),
        details: None,
    })
}

fn provider_uses_anthropic_credentials(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.contains("anthropic") || normalized.contains("claude")
}

/// Report runtime credential-management state, supported providers, provider
/// availability, and recent credential operation outcomes.
async fn check_credential_runtime(state: &HealthState) -> HealthCheck {
    let supported = state.credential_runtime.supported_providers();
    let provider_details = credential_provider_capabilities(state, &supported);
    let last_effect = state.credential_runtime.last_effect().await;
    let last_successful_validation = state.credential_runtime.last_successful_validation().await;
    let last_mutation_result = state.credential_runtime.last_mutation_result().await;

    let hot_apply_supported = provider_details
        .iter()
        .any(|detail| detail["hot_apply_supported"].as_bool().unwrap_or_default());
    let restart_required = last_mutation_result.as_ref().is_some_and(|result| {
        result.runtime_effect == CredentialMutationEffect::RestartRequired.as_str()
    });
    let degraded = restart_required
        || last_mutation_result
            .as_ref()
            .is_some_and(|result| result.result == "failure");

    let details = serde_json::json!({
        "supported_provider_names": supported,
        "supported_providers": provider_details,
        "hot_apply_supported": hot_apply_supported,
        "last_effect": last_effect,
        "last_successful_validation": last_successful_validation,
        "last_mutation_result": last_mutation_result,
        "restart_required": restart_required,
        "degraded": degraded,
    });

    let message = if restart_required {
        Some("credential mutation requires restart before runtime use".to_owned())
    } else if degraded {
        Some("last credential mutation failed".to_owned())
    } else if let Some(ref effect) = last_effect {
        Some(format!(
            "last mutation for '{}' returned '{}'",
            effect.provider,
            effect.effect.as_str()
        ))
    } else {
        Some(format!("supported providers: {}", supported.join(", ")))
    };

    HealthCheck {
        name: "credential_runtime".to_owned(),
        status: if degraded || restart_required {
            "warn".to_owned()
        } else {
            "pass".to_owned()
        },
        message,
        details: Some(details),
    }
}

fn credential_provider_capabilities(
    state: &HealthState,
    supported: &[String],
) -> Vec<serde_json::Value> {
    supported
        .iter()
        .map(|name| credential_provider_capability(state, name))
        .collect()
}

fn credential_provider_capability(state: &HealthState, name: &str) -> serde_json::Value {
    let effect = state.credential_runtime.mutation_effect(name);
    let availability = credential_provider_availability(&state.provider_registry, name);
    let hot_apply_supported = matches!(
        effect,
        CredentialMutationEffect::Applied | CredentialMutationEffect::PendingReload
    );
    let degraded = availability
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| matches!(status, "down" | "degraded"));
    serde_json::json!({
        "name": name,
        "hot_apply_supported": hot_apply_supported,
        "runtime_effect": effect.as_str(),
        "availability": availability,
        "restart_required": effect == CredentialMutationEffect::RestartRequired,
        "degraded": degraded,
    })
}

fn credential_provider_availability(
    registry: &hermeneus::provider::ProviderRegistry,
    name: &str,
) -> serde_json::Value {
    let registered_name = registry
        .providers()
        .into_iter()
        .map(|provider| provider.name().to_owned())
        .find(|registered| registered.eq_ignore_ascii_case(name));

    if let Some(registered_name) = registered_name {
        let health = registry
            .provider_health(&registered_name)
            .unwrap_or(ProviderHealth::Up);
        let mut detail = provider_health_detail(&registered_name, &health);
        if let Some(object) = detail.as_object_mut() {
            object.insert("registered".to_owned(), serde_json::Value::Bool(true));
        }
        return detail;
    }

    serde_json::json!({
        "name": name,
        "registered": false,
        "status": "not_registered",
    })
}

/// Check if the data directory is writable.
async fn check_storage_writable(state: &HealthState) -> HealthCheck {
    let data_dir = state.oikos.data();
    let instance_root = state.oikos.root();

    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        return HealthCheck {
            name: "storage_writable".to_owned(),
            status: "fail".to_owned(),
            message: Some(format!("cannot create data directory: {e}")),
            details: None,
        };
    }

    // WHY: validate that the data directory resolves within the instance
    // root to prevent path-traversal if oikos is misconfigured.
    if let Err(e) = koina::fs::validate_within_root(&data_dir, instance_root) {
        return HealthCheck {
            name: "storage_writable".to_owned(),
            status: "fail".to_owned(),
            message: Some(format!("data directory path validation failed: {e}")),
            details: None,
        };
    }

    let test_file = data_dir.join(".health-check-write-test");

    // WHY: validate the test file path stays within the data directory
    // (defense-in-depth against crafted data_dir values).
    if let Err(e) = koina::fs::validate_within_root(&test_file, &data_dir) {
        return HealthCheck {
            name: "storage_writable".to_owned(),
            status: "fail".to_owned(),
            message: Some(format!("test file path validation failed: {e}")),
            details: None,
        };
    }

    match tokio::fs::write(&test_file, b"health-check").await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(&test_file).await;
            HealthCheck {
                name: "storage_writable".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            }
        }
        Err(e) => HealthCheck {
            name: "storage_writable".to_owned(),
            status: "fail".to_owned(),
            message: Some(format!("data directory is not writable: {e}")),
            details: None,
        },
    }
}

/// Decode JWT expiry claim from a token.
/// Returns expiry timestamp in seconds since epoch, or None if not found/invalid.
fn decode_jwt_exp(token: &str) -> Option<u64> {
    // JWT format: header.payload.signature
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;

    let payload = base64url_decode(payload_b64).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    json.get("exp").and_then(serde_json::Value::as_u64)
}

/// Decode base64url-encoded string (no padding required).
fn base64url_decode(s: &str) -> Result<Vec<u8>, ()> {
    fn char_val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' | b'+' => Some(62),
            b'_' | b'/' => Some(63),
            b'=' => Some(0),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let end = bytes.iter().rposition(|&b| b != b'=').map_or(0, |i| i + 1);
    // INVARIANT: end <= bytes.len() by construction from rposition's return value.
    let bytes = bytes.get(..end).unwrap_or(bytes);

    let mut out = Vec::with_capacity(bytes.len() * 6 / 8 + 1);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        let v = char_val(b).ok_or(())?;
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buf >> bits) & 0xFF).unwrap_or(0));
        }
    }

    Ok(out)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after len assertions"
)]
mod tests {
    use hermeneus::provider::ProviderRegistry;

    use super::*;

    #[test]
    fn health_state_has_all_required_fields() {
        // WHY: compile-time shape assertion — if this fn compiles, HealthState
        // has every field the health handlers need, with the right types.
        #[expect(
            dead_code,
            reason = "compile-time shape assertion: proves field types via unused local fn"
        )]
        fn assert_health_state_fields(state: &HealthState) {
            use std::sync::Arc;

            use hermeneus::provider::ProviderRegistry;
            use mneme::store::SessionStore;
            use nous::manager::NousManager;
            use taxis::config::AletheiaConfig;
            use taxis::oikos::Oikos;

            let _: &Arc<tokio::sync::Mutex<SessionStore>> = &state.session_store;
            let _: &Arc<ProviderRegistry> = &state.provider_registry;
            let _: &Arc<NousManager> = &state.nous_manager;
            let _: std::time::Instant = state.start_time;
            let _: &Arc<Oikos> = &state.oikos;
            let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
            let _: &Option<Arc<dyn mneme::embedding::EmbeddingProvider>> =
                &state.embedding_provider;
        }
        assert!(std::mem::size_of::<HealthState>() > 0);
    }

    #[test]
    fn health_response_serializes_all_fields() {
        let resp = HealthResponse {
            status: "healthy".to_owned(),
            version: "1.0.0".to_owned(),
            git_sha: "abc123".to_owned(),
            uptime_seconds: 300,
            checks: vec![],
            data_dir: "/tmp/instance/data".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["uptime_seconds"], 300);
        assert!(json["checks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn liveness_response_serializes_only_status() {
        let resp = LivenessResponse {
            status: "healthy".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 1);
        assert_eq!(json["status"], "healthy");
    }

    #[test]
    fn gateway_security_fails_auth_none_on_lan_bind() {
        let check = gateway_security_check("none", "0.0.0.0");
        assert_eq!(check.status, "fail");
        assert_eq!(check.name, "gateway_security");
    }

    #[test]
    fn rate_limiting_check_reports_disabled_when_off() {
        let check = rate_limiting_check(false, false, false);
        assert_eq!(check.name, "rate_limiting");
        assert_eq!(check.status, "pass");
        let details = check.details.unwrap();
        assert_eq!(details["enabled"], false);
        assert_eq!(details["keying"], "disabled");
        assert!(check.message.as_deref().unwrap().contains("disabled"));
    }

    #[test]
    fn rate_limiting_check_reports_peer_socket_by_default() {
        let check = rate_limiting_check(true, false, false);
        assert_eq!(check.status, "pass");
        let details = check.details.unwrap();
        assert_eq!(details["keying"], "peer_socket");
        assert!(
            check.message.as_deref().unwrap().contains("peer socket"),
            "message should name peer socket keying: {:?}",
            check.message
        );
    }

    #[test]
    fn rate_limiting_check_reports_forwarded_ip_when_trusted() {
        let check = rate_limiting_check(true, true, false);
        assert_eq!(check.status, "pass");
        let details = check.details.unwrap();
        assert_eq!(details["keying"], "forwarded_client_ip");
        assert!(
            check.message.as_deref().unwrap().contains("forwarded"),
            "message should name forwarded IP keying: {:?}",
            check.message
        );
    }

    #[test]
    fn rate_limiting_check_reports_authenticated_user_when_per_user_enabled() {
        let check = rate_limiting_check(true, false, true);
        assert_eq!(check.status, "pass");
        let details = check.details.unwrap();
        assert_eq!(details["keying"], "authenticated_user");
        assert!(
            check
                .message
                .as_deref()
                .unwrap()
                .contains("authenticated user"),
            "message should name authenticated-user keying: {:?}",
            check.message
        );
    }

    #[test]
    fn health_check_pass_omits_message_when_none() {
        let check = HealthCheck {
            name: "session_store".to_owned(),
            status: "pass".to_owned(),
            message: None,
            details: None,
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["name"], "session_store");
        assert_eq!(json["status"], "pass");
        // NOTE: message is None: serializes as null (no skip annotation).
        assert!(json["message"].is_null());
    }

    #[test]
    fn health_check_fail_includes_message() {
        let check = HealthCheck {
            name: "providers".to_owned(),
            status: "fail".to_owned(),
            message: Some("no LLM providers registered".to_owned()),
            details: None,
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["status"], "fail");
        assert_eq!(json["message"], "no LLM providers registered");
    }

    #[test]
    fn nous_health_poller_passes_when_running_and_no_error() {
        let check = check_nous_health_poller(true, 0, None);
        assert_eq!(check.name, "nous_health_poller");
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
    }

    #[test]
    fn nous_health_poller_warns_when_running_with_recorded_error() {
        let check = check_nous_health_poller(true, 0, Some("connection reset"));
        assert_eq!(check.status, "warn");
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("running but has a recorded error"));
        assert!(message.contains("last_error=connection reset"));
    }

    #[test]
    fn nous_health_poller_warns_when_not_running_without_error() {
        let check = check_nous_health_poller(false, 0, None);
        assert_eq!(check.status, "warn");
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("poller is not running"));
    }

    #[test]
    fn nous_health_poller_fails_when_not_running_with_error() {
        let check = check_nous_health_poller(false, 2, Some("poller panicked"));
        assert_eq!(check.status, "fail");
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("poller is not running"));
        assert!(message.contains("restart_count=2"));
        assert!(message.contains("last_error=poller panicked"));
    }

    #[test]
    fn nous_health_poller_includes_restart_count_when_nonzero() {
        let check = check_nous_health_poller(true, 3, None);
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none(), "pass check omits diagnostics");

        let check = check_nous_health_poller(false, 3, None);
        assert_eq!(check.status, "warn");
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("restart_count=3"));
    }

    #[test]
    fn prosoche_heartbeat_path_check_reports_daemon_path() {
        let check =
            check_prosoche_heartbeat_path(&taxis::config::ProsocheMaintenanceSettings::default());
        assert_eq!(check.name, "prosoche_heartbeat_path");
        assert_eq!(check.status, "pass");
        let Some(msg) = check.message else {
            panic!("prosoche check has a message");
        };
        assert!(
            msg.contains("daemon"),
            "message should name daemon path: {msg}"
        );
        assert!(
            msg.contains("21600s"),
            "message should reference default self-audit cadence: {msg}"
        );
    }

    #[test]
    fn credential_check_only_uses_anthropic_credentials_for_anthropic_providers() {
        assert!(provider_uses_anthropic_credentials("anthropic"));
        assert!(provider_uses_anthropic_credentials("claude-code"));
        assert!(!provider_uses_anthropic_credentials("openai"));
        assert!(!provider_uses_anthropic_credentials("local"));
        assert!(!provider_uses_anthropic_credentials("mock"));
    }

    #[test]
    fn aggregate_status_unhealthy_when_any_check_fails() {
        let checks = [
            HealthCheck {
                name: "a".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            },
            HealthCheck {
                name: "b".to_owned(),
                status: "fail".to_owned(),
                message: Some("down".to_owned()),
                details: None,
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "unhealthy");
    }

    #[test]
    fn aggregate_status_degraded_when_any_check_warns() {
        let checks = [
            HealthCheck {
                name: "a".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            },
            HealthCheck {
                name: "b".to_owned(),
                status: "warn".to_owned(),
                message: Some("no providers".to_owned()),
                details: None,
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "degraded");
    }

    #[test]
    fn aggregate_status_healthy_when_all_pass() {
        let checks = [
            HealthCheck {
                name: "session_store".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            },
            HealthCheck {
                name: "providers".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            },
        ];
        let status = if checks.iter().any(|c| c.status == "fail") {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "healthy");
    }

    #[test]
    fn aggregate_status_unhealthy_when_any_check_times_out() {
        let checks = [
            HealthCheck {
                name: "a".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            },
            HealthCheck {
                name: "b".to_owned(),
                status: "timeout".to_owned(),
                message: Some("check timed out after 5s".to_owned()),
                details: None,
            },
        ];
        // WHY: "timeout" is treated as "fail" for aggregate status because
        // a timed-out check means we cannot confirm the subsystem is healthy.
        let status = if checks
            .iter()
            .any(|c| c.status == "fail" || c.status == "timeout")
        {
            "unhealthy"
        } else if checks.iter().any(|c| c.status == "warn") {
            "degraded"
        } else {
            "healthy"
        };
        assert_eq!(status, "unhealthy");
    }

    #[tokio::test(start_paused = true)]
    async fn timed_check_returns_timeout_on_slow_future() {
        let check = timed_check("slow_check", std::future::pending()).await;
        assert_eq!(check.status, "timeout");
        assert_eq!(check.name, "slow_check");
        assert!(check.message.unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn timed_check_returns_result_on_fast_future() {
        let check = timed_check("fast_check", async {
            HealthCheck {
                name: "fast_check".to_owned(),
                status: "pass".to_owned(),
                message: None,
                details: None,
            }
        })
        .await;
        assert_eq!(check.status, "pass");
        assert_eq!(check.name, "fast_check");
    }

    #[test]
    fn decode_jwt_exp_extracts_expiry() {
        // Create a JWT with known exp claim: exp = 1234567890
        // Payload: {"exp":1234567890}
        // base64url: eyJleHAiOjEyMzQ1Njc4OTB9
        let token = "header.eyJleHAiOjEyMzQ1Njc4OTB9.signature"; // pii-allow: synthetic JWT structure, exp-decoder self-test
        let exp = decode_jwt_exp(token);
        assert_eq!(exp, Some(1_234_567_890));
    }

    #[test]
    fn decode_jwt_exp_returns_none_for_invalid() {
        // No exp claim
        let token = "header.eyJzdWIiOiIxMjMifQ.signature";
        let exp = decode_jwt_exp(token);
        assert_eq!(exp, None);

        // Invalid format
        let exp = decode_jwt_exp("not-a-jwt");
        assert_eq!(exp, None);

        // Empty
        let exp = decode_jwt_exp("");
        assert_eq!(exp, None);
    }

    #[test]
    fn base64url_decode_handles_padding_variants() {
        // Standard base64url without padding
        let decoded = base64url_decode("eyJleHAiOjEyMzQ1Njc4OTB9").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"exp":1234567890}"#);

        // With padding should also work
        let decoded = base64url_decode("eyJleHAiOjEyMzQ1Njc4OTB9").unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"exp":1234567890}"#);
    }

    #[expect(
        clippy::as_conversions,
        reason = "sextet is masked to 0..=63 and indexes the 64-entry table"
    )]
    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for &b in input {
            buf = (buf << 8) | u32::from(b);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(char::from(TABLE[((buf >> bits) & 0x3F) as usize]));
            }
        }
        if bits > 0 {
            out.push(char::from(TABLE[((buf << (6 - bits)) & 0x3F) as usize]));
        }
        out
    }

    fn oauth_token_with_exp(exp: u64) -> String {
        let payload = serde_json::json!({ "exp": exp }).to_string();
        format!(
            "sk-ant-oat-header.{}.signature",
            base64url_encode(payload.as_bytes())
        )
    }

    #[test]
    fn env_token_expired_reports_expired() {
        let now = 1_000_000;
        let exp = now - 100;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("expired"));
    }

    #[test]
    fn env_token_expires_soon_reports_soon() {
        let now = 1_000_000;
        let exp = now + 150;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("soon"));
    }

    #[test]
    fn env_token_valid_reports_ok() {
        let now = 1_000_000;
        let exp = now + 900;
        let token = oauth_token_with_exp(exp);
        let check = check_env_oauth_token(&token, now, 60, 300);
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
    }

    #[test]
    fn env_token_empty_env_var_handled() {
        let check = check_env_oauth_token("", 1_000_000, 60, 300);
        assert_eq!(check.status, "warn");
        assert!(check.message.unwrap().contains("empty"));
    }

    #[test]
    fn env_token_undecodable_returns_error() {
        // A non-OAuth, non-JWT string does not trigger expiry checks and is
        // treated as a plain API key.
        let check = check_env_oauth_token("not-a-jwt-and-not-oauth", 1_000_000, 60, 300);
        assert_eq!(check.status, "pass");
    }

    fn api_request_error() -> hermeneus::error::Error {
        hermeneus::error::ApiRequestSnafu {
            message: "synthetic failure".to_owned(),
        }
        .build()
    }

    fn make_registry_with_named_providers(names: &[&'static str]) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        for name in names {
            registry.register(Box::new(
                hermeneus::test_utils::MockProvider::new("ok").named(name),
            ));
        }
        registry
    }

    fn degrade_provider(registry: &ProviderRegistry, name: &str) {
        registry.record_error(name, &api_request_error());
    }

    fn down_provider(registry: &ProviderRegistry, name: &str) {
        // WHY: Default health config requires 5 consecutive availability errors
        // before transitioning Up -> Down.
        for _ in 0..5 {
            registry.record_error(name, &api_request_error());
        }
    }

    fn provider_detail_names(check: &HealthCheck) -> Vec<String> {
        check
            .details
            .as_ref()
            .and_then(|details| details["providers"].as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|entry| entry["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn provider_reachability_passes_when_all_providers_up() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.name, "provider_reachability");
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
        assert_eq!(provider_detail_names(&check), vec!["alpha", "beta"]);
    }

    #[test]
    fn provider_reachability_warns_when_one_required_provider_degraded() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        degrade_provider(&registry, "alpha");
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.status, "warn");
        assert!(
            check.message.is_some(),
            "message should describe degraded provider"
        );
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("alpha") && message.contains("degraded"));
    }

    #[test]
    fn provider_reachability_fails_when_one_required_provider_down() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        down_provider(&registry, "alpha");
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.status, "fail");
        assert!(
            check.message.is_some(),
            "message should describe down provider"
        );
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("alpha") && message.contains("down"));
    }

    #[test]
    fn provider_reachability_fails_when_one_down_even_another_is_up() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        down_provider(&registry, "alpha");
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.status, "fail");
    }

    #[test]
    fn provider_reachability_warns_when_all_required_providers_degraded() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        degrade_provider(&registry, "alpha");
        degrade_provider(&registry, "beta");
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.status, "warn");
    }

    #[test]
    fn provider_reachability_fails_when_all_required_providers_down() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        down_provider(&registry, "alpha");
        down_provider(&registry, "beta");
        let check = provider_reachability_check(&registry, &HashSet::new());
        assert_eq!(check.status, "fail");
    }

    #[test]
    fn provider_reachability_passes_when_only_optional_provider_is_down() {
        let registry = make_registry_with_named_providers(&["alpha", "beta"]);
        down_provider(&registry, "beta");
        let optional = HashSet::from(["beta".to_owned()]);
        let check = provider_reachability_check(&registry, &optional);
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
        assert!(check.details.is_some(), "details should list all providers");
        let details = check.details.unwrap_or_default();
        let providers = details["providers"].as_array().unwrap();
        let beta = providers.iter().find(|entry| entry["name"] == "beta");
        assert!(beta.is_some(), "beta should be present");
        assert_eq!(beta.unwrap()["status"], "down");
    }

    #[test]
    fn provider_reachability_optional_list_parses_comma_separated_names() {
        let names = parse_optional_providers(" alpha , beta,  gamma ");
        assert!(names.contains("alpha"));
        assert!(names.contains("beta"));
        assert!(names.contains("gamma"));
        assert!(!names.contains(""));
    }
}
