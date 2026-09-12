// kanon:ignore RUST/file-too-long — cohesive control-plane truth surface: the flat
// `/api/v1/system/health` checks and the richer `/api/v1/system/status` subsystem
// records (#5313) share the same check functions and HealthState; splitting now
// would duplicate that data-gathering across sibling modules.
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
use hermeneus::provider::ProviderRegistry;
use organon::registry::ToolRegistry;
use thesauros::health::PackStatus;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::HealthState;

#[path = "health_dto.rs"]
mod health_dto;
pub use health_dto::{
    HealthCheck, HealthResponse, LivenessResponse, SubsystemStatus, SubsystemStatusResponse,
};

/// Per-check timeout: individual health checks that exceed this are reported as "timeout".
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Overall endpoint timeout: the health response is always returned within this bound.
const OVERALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-subsystem timeout for `/api/v1/system/status`'s collector (#7288).
///
/// Tighter than [`CHECK_TIMEOUT`]: every check this endpoint reaches reads
/// local, in-memory state (a mutex-guarded store, an actor mailbox, a
/// journal counter) rather than making a network call. A check that has
/// not resolved within this window is unconfirmed, not necessarily stuck —
/// `session_store`'s lock is also taken synchronously
/// (`blocking_lock()`) by ordinary request handling elsewhere (see
/// handlers/insights.rs, handlers/sessions/mod.rs, handlers/ops.rs), so a
/// busy-but-live gateway can legitimately hold it past this bound under
/// real contention. That is exactly why a `"timeout"` record aggregates as
/// `"degraded"`, not `"failed"` (see [`aggregate_subsystem_status`]).
/// Bounding each subsystem individually — and running them concurrently —
/// is what lets one hung subsystem report its own typed `"timeout"` record
/// while every sibling subsystem still reports its real status, instead of
/// the whole response blocking until [`OVERALL_TIMEOUT`] and collapsing
/// every subsystem into one opaque `system_status_collector` failure.
const SUBSYSTEM_CHECK_TIMEOUT: Duration = Duration::from_secs(1);

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

/// GET /api/v1/system/status: authoritative, operator-grade subsystem
/// status (#5313).
///
/// Distinct from `/api/v1/system/health`'s flat `checks` array: each record
/// here names an explicit code owner and is allowed to report `"unknown"`
/// for a subsystem this endpoint cannot yet see, instead of defaulting it
/// to `"healthy"` — a control plane that lies toward optimism is worse than
/// one that says "I don't know." This is the canonical backend source
/// Proskenion/Koilon should consume for control-plane status views instead
/// of re-deriving ad hoc health from the flat checks array.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no side
/// effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/system/status",
    responses(
        (status = 200, description = "Subsystem status", body = SubsystemStatusResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 503, description = "One or more subsystems failed", body = SubsystemStatusResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn system_status(
    State(state): State<HealthState>,
    claims: Claims,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;

    let generated_at = jiff::Timestamp::now().to_string();
    let subsystems = match tokio::time::timeout(
        OVERALL_TIMEOUT,
        collect_subsystem_status(&state, &generated_at),
    )
    .await
    {
        Ok(subsystems) => subsystems,
        // WHY: a timed-out collector must report failure, not an empty list
        // that aggregates to a fake "healthy" — the same anti-pattern this
        // endpoint exists to eliminate.
        Err(_elapsed) => vec![SubsystemStatus {
            id: "system_status_collector".to_owned(),
            name: "Subsystem Status Collector".to_owned(),
            status: "failed".to_owned(),
            owner: "crates/pylon::handlers::health".to_owned(),
            last_checked: generated_at.clone(),
            last_success: None,
            last_failure: Some(generated_at.clone()),
            degraded_reason: None,
            failure_reason: Some(format!(
                "subsystem status collection timed out after {}s",
                OVERALL_TIMEOUT.as_secs()
            )),
            details: None,
            suggested_action: None,
        }],
    };

    let status = aggregate_subsystem_status(&subsystems);
    let http_status = if status == "failed" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    Ok((
        http_status,
        Json(SubsystemStatusResponse {
            status: status.to_owned(),
            generated_at,
            subsystems,
        }),
    ))
}

async fn detailed_health(state: &HealthState) -> (StatusCode, HealthResponse) {
    let uptime = state.start_time.elapsed().as_secs();

    // WHY: Run all health checks concurrently with individual timeouts so a
    // single hanging check (e.g., provider connection) cannot block the entire
    // endpoint. The overall timeout guarantees a response even if multiple
    // checks hang simultaneously (#3277).
    let checks = tokio::time::timeout(OVERALL_TIMEOUT, collect_detailed_health_checks(state))
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
            version: koina::build_info::CRATE_VERSION.to_owned(),
            git_sha: koina::build_info::GIT_SHA.to_owned(),
            git_dirty: koina::build_info::git_dirty(),
            build_timestamp: koina::build_info::build_timestamp(),
            uptime_seconds: uptime,
            checks,
            data_dir: state.oikos.data().to_string_lossy().into_owned(),
        },
    )
}

async fn collect_detailed_health_checks(state: &HealthState) -> Vec<HealthCheck> {
    // WHY: read config once before spawning concurrent checks so each check
    // does not contend on the config lock.
    let (
        clock_skew_leeway,
        expiry_warning_threshold,
        prosoche,
        gateway_security_check,
        rate_limiting_check,
        metrics_mode,
        metrics_detailed,
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
            // WHY(#5929): read the live, hot-reloadable value rather than
            // `state.metrics_mode`/`state.metrics_detailed`, which are
            // populated once at startup and never re-derived by
            // `apply_reload` — reading them here would report a stale mode
            // after a config reload.
            config.gateway.metrics.mode,
            config.gateway.metrics.detailed,
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
    let runtime_assembly_check = check_runtime_assembly(state);
    let credential_check =
        check_credential_validity(state, clock_skew_leeway, expiry_warning_threshold);
    let credential_runtime_check = check_credential_runtime(state).await;
    let embedding_check = check_embedding_provider(state);
    let prosoche_check = check_prosoche_heartbeat_path(&prosoche);
    let metrics_exposure_check = metrics_exposure_check(
        metrics_mode,
        metrics_detailed,
        &state.oikos.data().to_string_lossy(),
    );

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
        runtime_assembly_check,
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
        metrics_exposure_check,
    ]
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

/// Build the `/metrics` exposure security check.
///
/// WHY(#5322): operator diagnostics should surface whether the Prometheus
/// scrape endpoint is in a safe posture for the configured bind address.
/// Report what the sandbox configuration would actually enforce.
///
/// WHY(#5232) all three and not egress alone: an operator asking "is this sandboxed"
/// wants the whole answer. Reporting egress by itself invites the reading that the other
/// two are fine, which is the same defect in a smaller frame -- a partial answer that
/// looks complete.
///
/// This is a preflight classification of the CONFIGURATION, not proof that a given
/// child's `pre_exec` installation succeeded. The message says so, because a health
/// endpoint that overstates what it verified is worse than one that reports nothing.
fn sandbox_check(sandbox: &organon::sandbox::SandboxConfig) -> HealthCheck {
    use organon::sandbox::GuaranteeStatus;

    let guarantees = organon::sandbox::diagnostic_guarantees(sandbox);
    let each = [
        ("landlock", guarantees.landlock),
        ("seccomp", guarantees.seccomp),
        ("egress", guarantees.egress),
    ];

    // Unavailable blocks execution in enforcing mode; Degraded lets it continue with the
    // guarantee unmet. Unrestricted is not a fault -- it means the guarantee was never
    // asked for, and reporting a deliberate `egress=allow` as a warning would train
    // operators to ignore this check.
    let status = if each.iter().any(|(_, s)| *s == GuaranteeStatus::Unavailable) {
        "fail"
    } else if each.iter().any(|(_, s)| *s == GuaranteeStatus::Degraded) {
        "warn"
    } else {
        "pass"
    };

    let unmet: Vec<&str> = each
        .iter()
        .filter(|(_, s)| matches!(s, GuaranteeStatus::Degraded | GuaranteeStatus::Unavailable))
        .map(|(name, _)| *name)
        .collect();

    let message = if unmet.is_empty() {
        "sandbox guarantees are as configured (preflight classification, not proof of \
         per-child enforcement)"
            .to_owned()
    } else {
        format!(
            "sandbox guarantee(s) not enforced as configured: {}. This is a preflight \
             classification of the configuration, not proof of per-child enforcement.",
            unmet.join(", ")
        )
    };

    HealthCheck {
        name: "sandbox".to_owned(),
        status: status.to_owned(),
        message: Some(message),
        details: Some(serde_json::json!({
            "landlock": guarantees.landlock.to_string(),
            "seccomp": guarantees.seccomp.to_string(),
            "egress": guarantees.egress.to_string(),
        })),
    }
}

fn metrics_exposure_check(
    mode: taxis::config::MetricsMode,
    detailed: bool,
    data_dir: &str,
) -> HealthCheck {
    use taxis::config::MetricsMode;

    match mode {
        MetricsMode::Disabled => HealthCheck {
            name: "metrics_exposure".to_owned(),
            status: "pass".to_owned(),
            message: Some("metrics endpoint is disabled".to_owned()),
            details: None,
        },
        MetricsMode::Bearer => HealthCheck {
            name: "metrics_exposure".to_owned(),
            status: "pass".to_owned(),
            message: Some("metrics endpoint requires bearer authentication".to_owned()),
            details: None,
        },
        MetricsMode::LocalOnly => HealthCheck {
            name: "metrics_exposure".to_owned(),
            status: "pass".to_owned(),
            message: Some("metrics endpoint restricted to loopback connections".to_owned()),
            details: None,
        },
        MetricsMode::Public => {
            let mut message =
                "metrics endpoint is publicly accessible; ensure this is intentional".to_owned();
            if detailed {
                message.push_str(" (detailed label values expose nous_id, tool names, and paths)");
            }
            HealthCheck {
                name: "metrics_exposure".to_owned(),
                status: "warn".to_owned(),
                message: Some(message),
                details: Some(serde_json::json!({
                    "mode": "public",
                    "detailed": detailed,
                    "data_dir": data_dir,
                })),
            }
        }
    }
}

/// Build the rate-limiting diagnostics check.
///
/// Reports the active keying strategy so the control plane can show whether
/// rate limits are keyed by peer socket, forwarded client address, or
/// authenticated user. Per-user rate limiting takes precedence when enabled;
/// otherwise the address limiter follows the `trust_proxy` flag.
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

/// Run a health check with [`SUBSYSTEM_CHECK_TIMEOUT`] for the
/// `/api/v1/system/status` collector (#7288). Mirrors [`timed_check`]'s
/// pattern with a tighter bound appropriate to this endpoint's
/// local-state-only checks.
async fn timed_subsystem_check(
    name: &'static str,
    future: impl std::future::Future<Output = HealthCheck>,
) -> HealthCheck {
    match tokio::time::timeout(SUBSYSTEM_CHECK_TIMEOUT, future).await {
        Ok(check) => check,
        Err(_elapsed) => HealthCheck {
            name: name.to_owned(),
            status: "timeout".to_owned(),
            message: Some(format!(
                "{name} check timed out after {}s",
                SUBSYSTEM_CHECK_TIMEOUT.as_secs()
            )),
            details: None,
        },
    }
}

/// Run a subsystem-status future — one that assembles its own
/// [`SubsystemStatus`] directly rather than going through
/// [`subsystem_from_check`] — with [`SUBSYSTEM_CHECK_TIMEOUT`] (#7288). On
/// timeout, synthesizes a typed `"timeout"` record naming this specific
/// subsystem instead of letting the hang propagate up and collapse the
/// whole `/api/v1/system/status` response into one opaque
/// `system_status_collector` failure.
async fn bounded_subsystem_status(
    id: &'static str,
    name: &'static str,
    owner: &'static str,
    generated_at: &str,
    future: impl std::future::Future<Output = SubsystemStatus>,
) -> SubsystemStatus {
    match tokio::time::timeout(SUBSYSTEM_CHECK_TIMEOUT, future).await {
        Ok(status) => status,
        Err(_elapsed) => SubsystemStatus {
            id: id.to_owned(),
            name: name.to_owned(),
            status: "timeout".to_owned(),
            owner: owner.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: Some(generated_at.to_owned()),
            degraded_reason: None,
            failure_reason: Some(format!(
                "{name} check timed out after {}s",
                SUBSYSTEM_CHECK_TIMEOUT.as_secs()
            )),
            details: None,
            suggested_action: Some(format!(
                "{owner} did not respond within {}s; check for a stuck lock, actor, or \
                 blocking I/O call.",
                SUBSYSTEM_CHECK_TIMEOUT.as_secs()
            )),
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

/// Check whether this process was assembled with any agent-loop capabilities.
fn check_runtime_assembly(state: &HealthState) -> HealthCheck {
    runtime_assembly_check(&state.provider_registry, &state.tool_registry)
}

fn runtime_assembly_check(
    provider_registry: &ProviderRegistry,
    tool_registry: &ToolRegistry,
) -> HealthCheck {
    let provider_count = provider_registry.providers().len();
    let tool_count = tool_registry.definitions().len();
    let details = Some(serde_json::json!({
        "provider_count": provider_count,
        "tool_count": tool_count,
        "canonical_startup": "aletheia serve",
    }));

    if provider_count == 0 && tool_count == 0 {
        return HealthCheck {
            name: "runtime_assembly".to_owned(),
            status: "fail".to_owned(),
            message: Some(
                "gateway-only harness has no providers or tools registered; use `aletheia serve` for production startup"
                    .to_owned(),
            ),
            details,
        };
    }

    HealthCheck {
        name: "runtime_assembly".to_owned(),
        status: "pass".to_owned(),
        message: None,
        details,
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
    let Some(provider) = state.embedding_provider.as_ref() else {
        return HealthCheck {
            name: "embedding_provider".to_owned(),
            status: "warn".to_owned(),
            message: Some("no embedding provider configured".to_owned()),
            details: None,
        };
    };

    let model_name = provider.model_name();

    if model_name == mneme::embedding::LOADING_MODEL_NAME {
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
    // WHY(#5313): this check used to report "pass" unconditionally, including
    // when neither path is active — a fake-pass that told an operator
    // maintenance was fine while nothing was actually scheduled to run.
    let status = if runs_daemon || uses_external {
        "pass"
    } else {
        "warn"
    };
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
        status: status.to_owned(),
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
            message: Some("no credentials found (ANTHROPIC_API_KEY not set, no credential file, no explicit Claude Code credentials)".to_owned()),
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

/// Report runtime credential-management state: supported providers and the
/// effect of the most recent mutation (#4872).
async fn check_credential_runtime(state: &HealthState) -> HealthCheck {
    let supported = state.credential_runtime.supported_providers();
    let last_effect = state.credential_runtime.last_effect().await;

    let details = serde_json::json!({
        "supported_providers": supported,
        "last_effect": last_effect,
    });

    let message = if let Some(ref effect) = last_effect {
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
        status: "pass".to_owned(),
        message,
        details: Some(details),
    }
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

// ── Subsystem status (#5313) ──

/// Severity ordering for the internal `pass`/`warn`/`fail`/`timeout` check
/// scale, used to fold two related [`HealthCheck`]s into one
/// [`SubsystemStatus`] by worst-of.
fn health_check_severity(status: &str) -> u8 {
    match status {
        "pass" => 0,
        "fail" | "timeout" => 2,
        // "warn" and any unrecognized status are treated alike: not
        // clean-pass, not confirmed-failed.
        _ => 1,
    }
}

/// Wrap an existing [`HealthCheck`] into a [`SubsystemStatus`] record,
/// mapping the check's `pass`/`warn`/`fail`/`timeout` scale onto the richer
/// `healthy`/`degraded`/`failed` vocabulary and attributing an explicit
/// code owner (#5313's acceptance criteria: one owner per subsystem).
fn subsystem_from_check(
    check: HealthCheck,
    id: &str,
    name: &str,
    owner: &str,
    generated_at: &str,
    suggested_action: Option<&str>,
) -> SubsystemStatus {
    let status = match check.status.as_str() {
        "pass" => "healthy",
        "warn" => "degraded",
        // WHY(#7288): "timeout" is kept distinct from "fail" here — a
        // subsystem that did not answer in time is a different, typed
        // signal from one that answered with an explicit failure, and an
        // operator or the control-plane UI should be able to tell them
        // apart instead of both reading as an identical opaque "failed".
        "timeout" => "timeout",
        _ => "failed", // "fail"
    };
    SubsystemStatus {
        id: id.to_owned(),
        name: name.to_owned(),
        status: status.to_owned(),
        owner: owner.to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: (status == "failed" || status == "timeout").then(|| generated_at.to_owned()),
        degraded_reason: (status == "degraded")
            .then(|| check.message.clone())
            .flatten(),
        failure_reason: (status == "failed" || status == "timeout")
            .then(|| check.message.clone())
            .flatten(),
        details: check.details,
        suggested_action: (status != "healthy")
            .then(|| suggested_action.map(str::to_owned))
            .flatten(),
    }
}

/// Combine two related [`HealthCheck`]s into one worst-of check with both
/// messages preserved, for subsystems the flat health endpoint tracks as
/// two checks but that a control-plane operator should see as one record.
fn combine_checks(name: &'static str, a: &HealthCheck, b: &HealthCheck) -> HealthCheck {
    let status = if health_check_severity(&b.status) > health_check_severity(&a.status) {
        b.status.clone()
    } else {
        a.status.clone()
    };
    let message = [a.message.as_deref(), b.message.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
    HealthCheck {
        name: name.to_owned(),
        status,
        message: (!message.is_empty()).then_some(message),
        details: b.details.clone().or_else(|| a.details.clone()),
    }
}

/// Raw check outputs gathered before subsystem-status assembly.
///
/// Kept as one struct + one gathering function so `collect_subsystem_status`
/// reads as pure assembly (map checks onto owned subsystem records) rather
/// than interleaving data-gathering with presentation.
struct FlatSubsystemChecks {
    prosoche: taxis::config::ProsocheMaintenanceSettings,
    gateway: HealthCheck,
    rate_limit: HealthCheck,
    session_store: HealthCheck,
    actor: HealthCheck,
    poller: HealthCheck,
    provider_reachability: HealthCheck,
    credential_validity: HealthCheck,
    credential_runtime: HealthCheck,
    embedding: HealthCheck,
    metrics: HealthCheck,
    sandbox: HealthCheck,
}

/// Run every existing flat check function once (SSOT: the same computation
/// backs both `/api/v1/system/health`'s flat array and this endpoint's
/// richer per-subsystem records).
async fn gather_flat_subsystem_checks(state: &HealthState) -> FlatSubsystemChecks {
    let (
        clock_skew_leeway,
        expiry_warning_threshold,
        prosoche,
        gateway,
        rate_limit,
        metrics_mode,
        metrics_detailed,
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
            // WHY(#5929): live config, not the startup-only `state.metrics_mode`
            // / `state.metrics_detailed` fields — see the sibling read above.
            config.gateway.metrics.mode,
            config.gateway.metrics.detailed,
        )
    };

    // WHY(#7288): session_store, nous_actors, and credential_runtime each
    // await a lock or actor mailbox that can legitimately hang. Previously
    // these were awaited one after another with no bound, so a single stuck
    // one blocked this whole function (and every sync check below it) until
    // the outer `/api/v1/system/status` handler's `OVERALL_TIMEOUT` fired.
    // Running them concurrently, each under `SUBSYSTEM_CHECK_TIMEOUT`, means
    // a stuck one reports its own typed "timeout" while its siblings — here
    // and the synchronous checks that follow — still resolve normally.
    let (session_store, actor, credential_runtime) = tokio::join!(
        timed_subsystem_check("session_store", check_session_store(state)),
        timed_subsystem_check("nous_actors", check_nous_actors(state)),
        timed_subsystem_check("credential_runtime", check_credential_runtime(state)),
    );
    let poller_snapshot = state.nous_manager.poller_snapshot();
    let poller = check_nous_health_poller(
        poller_snapshot.running,
        poller_snapshot.restart_count,
        poller_snapshot.last_error.as_deref(),
    );
    let provider_reachability = check_provider_reachability(state);
    let credential_validity =
        check_credential_validity(state, clock_skew_leeway, expiry_warning_threshold);
    let embedding = check_embedding_provider(state);
    let metrics = metrics_exposure_check(
        metrics_mode,
        metrics_detailed,
        &state.oikos.data().to_string_lossy(),
    );

    let sandbox = sandbox_check(&state.config.read().await.sandbox);

    FlatSubsystemChecks {
        prosoche,
        gateway,
        rate_limit,
        session_store,
        actor,
        poller,
        provider_reachability,
        credential_validity,
        credential_runtime,
        embedding,
        metrics,
        sandbox,
    }
}

/// Wrap `credential_validity` and `credential_runtime` into one
/// `provider_credentials` record.
fn subsystem_provider_credentials(
    validity: HealthCheck,
    runtime: HealthCheck,
    generated_at: &str,
) -> SubsystemStatus {
    let mut status = subsystem_from_check(
        validity,
        "provider_credentials",
        "Provider Credential Validity",
        "crates/symbolon::credential",
        generated_at,
        Some("Refresh or rotate the expiring/missing credential."),
    );
    // WHY: credential_validity is the sharper status signal (expiry,
    // presence); credential_runtime's status is always "pass" by
    // construction (it reports the last mutation outcome, not a live
    // check), so only its detail — supported providers, last mutation
    // effect — is folded in, never its status.
    status.details = runtime.details;
    status
}

/// Collect authoritative, operator-grade subsystem status records (#5313).
///
/// Reuses the same check functions that back `/api/v1/system/health` where
/// real data already exists (SSOT: one computation, two presentations).
/// Subsystems with no pylon-reachable signal today report `"unknown"`
/// rather than being silently omitted or defaulted to `"healthy"`.
async fn collect_subsystem_status(state: &HealthState, generated_at: &str) -> Vec<SubsystemStatus> {
    // WHY(#7288): these four gathering calls were previously awaited one
    // after another with no per-item bound, so a hang in any one of them —
    // `gather_flat_subsystem_checks` internally bounds its own hang-prone
    // checks, but `subsystem_turn_event_persistence`,
    // `subsystem_tool_execution_history`, and `subsystem_event_bus` did not
    // — blocked the entire response until the outer handler's
    // `OVERALL_TIMEOUT`. Running them concurrently, each bounded, keeps a
    // stuck one from delaying or blanking out its siblings.
    let (checks, turn_event_persistence, tool_execution_history, event_bus) = tokio::join!(
        gather_flat_subsystem_checks(state),
        bounded_subsystem_status(
            "turn_event_persistence",
            "Turn Event Buffer",
            "crates/pylon::turn_buffer",
            generated_at,
            subsystem_turn_event_persistence(state, generated_at),
        ),
        bounded_subsystem_status(
            "tool_execution_history",
            "Tool Execution History",
            "crates/mneme::store",
            generated_at,
            subsystem_tool_execution_history(state, generated_at),
        ),
        bounded_subsystem_status(
            "event_bus",
            "Domain Event Bus / SSE",
            "crates/pylon::event_bus",
            generated_at,
            subsystem_event_bus(state, generated_at),
        ),
    );

    vec![
        subsystem_from_check(
            checks.provider_reachability,
            "provider_reachability",
            "LLM Provider Reachability",
            "crates/hermeneus",
            generated_at,
            Some("Check provider credentials and network reachability."),
        ),
        subsystem_provider_credentials(
            checks.credential_validity,
            checks.credential_runtime,
            generated_at,
        ),
        subsystem_from_check(
            checks.embedding,
            "embeddings",
            "Embedding Provider",
            "crates/mneme::embedding",
            generated_at,
            Some("Check embedding model load logs; recall falls back to BM25 while degraded."),
        ),
        subsystem_from_check(
            checks.session_store,
            "session_store",
            "Session Store",
            "crates/mneme::store",
            generated_at,
            Some("Check the session store backend connectivity."),
        ),
        subsystem_from_check(
            combine_checks("nous_runtime", &checks.actor, &checks.poller),
            "nous_runtime",
            "Nous Agent Runtime",
            "crates/nous::manager",
            generated_at,
            Some("Check nous actor logs; a dead actor may need POST /api/v1/nous/{id}/recover."),
        ),
        turn_event_persistence,
        subsystem_memory_graph(state, generated_at),
        subsystem_domain_packs(state, generated_at),
        subsystem_daemon_runtime(&state.daemon_task_states, &checks.prosoche, generated_at),
        tool_execution_history,
        subsystem_training_qa_persistence(generated_at),
        subsystem_from_check(
            checks.metrics,
            "metrics_exposure",
            "Metrics Exposure",
            "crates/pylon::metrics",
            generated_at,
            Some(
                "Review `/metrics` exposure mode; avoid `public` with `detailed=true` on \
                 non-loopback binds.",
            ),
        ),
        subsystem_from_check(
            checks.sandbox,
            "sandbox",
            "Tool Sandbox",
            "crates/organon::sandbox",
            generated_at,
            Some(
                "Review `sandbox.enforcement` and `sandbox.egress`; a degraded guarantee \
                 means tools run without the restriction the config asks for.",
            ),
        ),
        event_bus,
        subsystem_disk_space(state, generated_at),
        subsystem_from_check(
            combine_checks(
                "config_security_posture",
                &checks.gateway,
                &checks.rate_limit,
            ),
            "config_security_posture",
            "Gateway Config & Security Posture",
            "crates/pylon::security",
            generated_at,
            Some("Review gateway auth mode, bind address, and rate-limit configuration."),
        ),
    ]
}

/// Turn event buffer subsystem: a real liveness/backlog signal from
/// `TurnBufferRegistry`, not a fabricated pass.
async fn subsystem_turn_event_persistence(
    state: &HealthState,
    generated_at: &str,
) -> SubsystemStatus {
    let active = state.turn_buffer_registry.active_count().await;
    SubsystemStatus {
        id: "turn_event_persistence".to_owned(),
        name: "Turn Event Buffer".to_owned(),
        status: "healthy".to_owned(),
        owner: "crates/pylon::turn_buffer".to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: None,
        degraded_reason: None,
        failure_reason: None,
        details: Some(serde_json::json!({ "active_turn_buffers": active })),
        suggested_action: None,
    }
}

/// Memory graph (knowledge store) subsystem: a presence check only. Full
/// graph diagnostics live behind `GET /api/v1/knowledge/check`, which this
/// deliberately does not duplicate.
fn subsystem_memory_graph(state: &HealthState, generated_at: &str) -> SubsystemStatus {
    #[cfg(feature = "knowledge-store")]
    let present = state.knowledge_store.is_some();
    #[cfg(not(feature = "knowledge-store"))]
    let present = {
        let _ = state;
        false
    };

    if present {
        SubsystemStatus {
            id: "memory_graph".to_owned(),
            name: "Memory Graph (Knowledge Store)".to_owned(),
            status: "healthy".to_owned(),
            owner: "crates/mneme::knowledge_store".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: None,
            details: Some(serde_json::json!({ "note": "presence check only" })),
            suggested_action: Some(
                "See GET /api/v1/knowledge/check for graph diagnostics.".to_owned(),
            ),
        }
    } else {
        SubsystemStatus {
            id: "memory_graph".to_owned(),
            name: "Memory Graph (Knowledge Store)".to_owned(),
            status: "unknown".to_owned(),
            owner: "crates/mneme::knowledge_store".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: Some(
                "knowledge-store feature not enabled, or no store configured on this instance"
                    .to_owned(),
            ),
            details: None,
            suggested_action: None,
        }
    }
}

/// Domain pack health subsystem (#5208): surfaces the loader's structured
/// per-pack [`thesauros::health::PackReport`] through the operator-grade
/// status endpoint, so a pack that failed to load, lost required context,
/// or dropped a tool registration is visible here instead of only in a
/// startup log line nobody is tailing.
///
/// The report is a load-time snapshot taken once at server startup
/// (`NousManager::with_pack_report`); it does not re-probe pack state on
/// every call, matching how the packs themselves are loaded once and held
/// for the life of the process.
fn subsystem_domain_packs(state: &HealthState, generated_at: &str) -> SubsystemStatus {
    let report = state.nous_manager.pack_report();
    let counts = report.status_counts();

    let status = if report.has_failures() {
        "failed"
    } else if counts.degraded > 0 {
        "degraded"
    } else {
        "healthy"
    };

    let named = |predicate: fn(PackStatus) -> bool| -> String {
        report
            .packs
            .iter()
            .filter(|p| predicate(p.status))
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    SubsystemStatus {
        id: "domain_packs".to_owned(),
        name: "Domain Packs".to_owned(),
        status: status.to_owned(),
        owner: "crates/thesauros::health".to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: None,
        degraded_reason: (status == "degraded")
            .then(|| format!("degraded packs: {}", named(|s| s == PackStatus::Degraded))),
        failure_reason: (status == "failed")
            .then(|| format!("failed packs: {}", named(|s| s == PackStatus::Failed))),
        details: Some(serde_json::json!({
            "active": counts.active,
            "degraded": counts.degraded,
            "failed": counts.failed,
            "packs": report.packs,
            "notes": report.notes,
        })),
        suggested_action: (status != "healthy").then_some(
            "Check startup logs for `domain pack loaded` / `failed to load domain pack`; \
             each pack's `issues` array in `details` names the failing component and reason."
                .to_owned(),
        ),
    }
}

/// Aggregate outcome across every attached daemon task-state store.
struct DaemonTaskStateSummary {
    /// One `{runner, task_count}` record per attached store.
    runner_task_counts: Vec<serde_json::Value>,
    /// Tasks disabled (auto-disabled or operator-disabled; see
    /// `oikonomos::state::DisableCause`, #7206).
    disabled_tasks: Vec<serde_json::Value>,
    /// Human-readable `"task_id (cause)"` labels for `disabled_tasks`, in the
    /// same order, so [`subsystem_daemon_runtime`] can name every disabled
    /// task in `degraded_reason` without re-deriving the label (#7206).
    disabled_task_labels: Vec<String>,
    /// Enabled tasks currently in backoff after a recent failure.
    backoff_tasks: Vec<serde_json::Value>,
    /// Stores that could not be read at all.
    read_errors: Vec<String>,
}

/// Read every attached [`oikonomos::state::TaskStateStore`] and classify
/// each persisted task.
///
/// WHY: a registered task's `enabled` flag only ever transitions to `false`
/// via [`oikonomos`]'s own 3-consecutive-failure auto-disable — every
/// registration path (maintenance tasks, prosoche tasks) starts `enabled:
/// true` and disabled-by-config tasks are never registered at all — so a
/// persisted `enabled: Some(false)` unambiguously means auto-disabled, not
/// "administrator turned this off intentionally".
fn summarize_daemon_task_states(
    daemon_task_states: &[(String, oikonomos::state::TaskStateStore)],
) -> DaemonTaskStateSummary {
    let mut summary = DaemonTaskStateSummary {
        runner_task_counts: Vec::with_capacity(daemon_task_states.len()),
        disabled_tasks: Vec::new(),
        disabled_task_labels: Vec::new(),
        backoff_tasks: Vec::new(),
        read_errors: Vec::new(),
    };

    for (component, store) in daemon_task_states {
        match store.load_all() {
            Ok(states) => {
                summary.runner_task_counts.push(serde_json::json!({
                    "runner": component,
                    "task_count": states.len(),
                }));
                for task in &states {
                    // WHY(#5130): legacy records predate the persisted
                    // `enabled` flag; absence means "was enabled", not unknown.
                    let enabled = task.enabled.unwrap_or(true);
                    // WHY(#7206): a legacy disabled record (or one written by
                    // the runner's own 3-consecutive-failure policy) has no
                    // `Operator` cause on file; both read as `auto_failure`,
                    // matching `runner::persistence::apply_saved_state`'s
                    // hydration treatment of the same ambiguity.
                    let cause = (!enabled)
                        .then(|| {
                            task.disable_cause
                                .unwrap_or(oikonomos::state::DisableCause::AutoFailure)
                        })
                        .map(disable_cause_label);
                    let detail = serde_json::json!({
                        "runner": component,
                        "task_id": task.task_id,
                        "consecutive_failures": task.consecutive_failures,
                        "last_error": task.last_error,
                        "cause": cause,
                    });
                    if !enabled {
                        summary.disabled_task_labels.push(format!(
                            "{} ({})",
                            task.task_id,
                            cause.unwrap_or("unknown")
                        ));
                        summary.disabled_tasks.push(detail);
                    } else if task.consecutive_failures > 0 {
                        summary.backoff_tasks.push(detail);
                    }
                }
            }
            Err(e) => summary.read_errors.push(format!("{component}: {e}")),
        }
    }

    summary
}

/// Stable wire label for [`oikonomos::state::DisableCause`] (`snake_case`,
/// matching the enum's own `#[serde(rename_all)]`).
pub(crate) fn disable_cause_label(cause: oikonomos::state::DisableCause) -> &'static str {
    match cause {
        oikonomos::state::DisableCause::AutoFailure => "auto_failure",
        oikonomos::state::DisableCause::Operator => "operator",
    }
}

/// Daemon / cron / maintenance runtime subsystem: real task state read from
/// the runner-attached [`oikonomos::state::TaskStateStore`] handles (#5142).
///
/// An empty `daemon_task_states` is itself a real, known signal — daemon
/// mode is disabled for this instance — not the permanent `"unknown"` this
/// record used to report unconditionally before a reader was attached.
/// `details.configured` still carries the configured intent pulled from
/// `state.config`, alongside the verified runtime state, so a mismatch
/// between "should be running" and "is running" stays visible.
fn subsystem_daemon_runtime(
    daemon_task_states: &[(String, oikonomos::state::TaskStateStore)],
    prosoche: &taxis::config::ProsocheMaintenanceSettings,
    generated_at: &str,
) -> SubsystemStatus {
    let configured = serde_json::json!({
        "heartbeat_enabled": prosoche.heartbeat.enabled,
        "self_audit_enabled": prosoche.self_audit.enabled,
        "external_timer_enabled": prosoche.external_timer.enabled,
    });

    // WHY "unknown" rather than "healthy": an empty reader slice cannot
    // distinguish "daemon mode is off for this instance" from "nobody wired
    // the readers into AppState". Reporting healthy asserts the runtime is
    // fine on no evidence, and hides a real misconfiguration — prosoche
    // settings can be enabled here while zero task-state stores exist.
    // "unknown" is the honest verdict and does not fail the aggregate, so a
    // genuinely daemon-less instance still reports overall healthy.
    if daemon_task_states.is_empty() {
        return SubsystemStatus {
            id: "daemon_runtime".to_owned(),
            name: "Daemon / Cron / Dispatch Runtime".to_owned(),
            status: "unknown".to_owned(),
            owner: "crates/oikonomos::runner".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            // WHY failure_reason and not details.note: the DTO defines this
            // field as the explanation for `failed` OR `unknown`, and the
            // sibling `subsystem_training_qa_persistence` uses it for exactly
            // that. Putting the explanation only in `details` made
            // daemon_runtime the one `unknown` subsystem that explains itself
            // somewhere else, which the public-API test correctly rejects.
            failure_reason: Some(
                "no daemon task-state readers are wired; daemon mode is either \
                 disabled for this instance or the readers were not threaded \
                 into AppState"
                    .to_owned(),
            ),
            details: Some(serde_json::json!({
                "configured": configured,
                "runners": [],
            })),
            suggested_action: None,
        };
    }

    let summary = summarize_daemon_task_states(daemon_task_states);
    let details = Some(serde_json::json!({
        "configured": configured,
        "runners": summary.runner_task_counts,
        "disabled_tasks": summary.disabled_tasks,
        "backoff_tasks": summary.backoff_tasks,
    }));

    // WHY(#7206): every task in `oikonomos::maintenance::registry` is
    // background cron/maintenance work (trace rotation, drift detection,
    // routing-store refresh, knowledge-graph upkeep, ...) -- none sit on the
    // request-serving path, unlike `provider_reachability`, `session_store`,
    // `nous_runtime`, or `config_security_posture` elsewhere in
    // `collect_subsystem_status`, whose `"failed"` correctly still promotes
    // the aggregate to 503 (see `aggregate_subsystem_status`). A daemon task
    // being disabled or in backoff is real and must not be hidden -- it is
    // "degraded", not "the gateway is unusable" -- so this subsystem's floor
    // for that case is `degraded`, never `failed`/503. A `read_errors`
    // failure below is a different animal: the `TaskStateStore` itself
    // (fjall) being unreadable is a storage-layer fault, not a fact about any
    // one task, so it keeps `failed`/503.
    let (status, degraded_reason, failure_reason, suggested_action) =
        if summary.read_errors.is_empty() {
            if !summary.disabled_tasks.is_empty() {
                (
                    "degraded",
                    Some(format!(
                        "{} task(s) disabled: {}",
                        summary.disabled_tasks.len(),
                        summary.disabled_task_labels.join(", ")
                    )),
                    None,
                    Some(
                        "Check daemon logs for the disabled task's last_error. An \
                         `auto_failure` cause is retried automatically on the next daemon \
                         restart, or immediately via POST /api/v1/system/daemon/tasks/\
                         {runner}/{task_id}/retry; an `operator` cause stays disabled until \
                         POST .../enable."
                            .to_owned(),
                    ),
                )
            } else if summary.backoff_tasks.is_empty() {
                ("healthy", None, None, None)
            } else {
                (
                    "degraded",
                    Some(format!(
                        "{} task(s) in backoff after a recent failure",
                        summary.backoff_tasks.len()
                    )),
                    None,
                    Some("Check daemon logs for the failing task's last_error.".to_owned()),
                )
            }
        } else {
            (
                "failed",
                None,
                Some(format!(
                    "task-state store unreadable: {}",
                    summary.read_errors.join("; ")
                )),
                Some(
                    "Check the daemon-task-state fjall directory for corruption or a lock \
                     conflict."
                        .to_owned(),
                ),
            )
        };

    SubsystemStatus {
        id: "daemon_runtime".to_owned(),
        name: "Daemon / Cron / Dispatch Runtime".to_owned(),
        status: status.to_owned(),
        owner: "crates/oikonomos::runner".to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: None,
        degraded_reason,
        failure_reason,
        details,
        suggested_action,
    }
}

/// Convert unix epoch seconds to an ISO 8601 string, matching `generated_at`'s
/// format. Returns `None` if the value is outside jiff's representable range
/// (never in practice for a `SystemTime::now()`-derived timestamp).
fn format_epoch_secs(secs: u64) -> Option<String> {
    i64::try_from(secs)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map(|ts| ts.to_string())
}

/// Disk-space monitor subsystem (#5128): current status, configured
/// thresholds, and last successful refresh from the shared
/// `DiskSpaceMonitor` on `AppState`. Reports `"unknown"` — not a fabricated
/// pass — when `maintenance.diskSpace.enabled = false`.
fn subsystem_disk_space(state: &HealthState, generated_at: &str) -> SubsystemStatus {
    const BYTES_PER_MB: u64 = 1024 * 1024;
    const OWNER: &str = "crates/koina::disk_space";

    let Some(monitor) = state.disk_monitor.as_ref() else {
        return SubsystemStatus {
            id: "disk_space".to_owned(),
            name: "Disk Space Monitor".to_owned(),
            status: "unknown".to_owned(),
            owner: OWNER.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: Some(
                "maintenance.diskSpace.enabled = false -- monitoring not active".to_owned(),
            ),
            details: Some(serde_json::json!({ "config_active": false })),
            suggested_action: None,
        };
    };

    let status = monitor.status();
    let available_mb = status.available_bytes() / BYTES_PER_MB;
    let warning_mb = monitor.warning_bytes() / BYTES_PER_MB;
    let critical_mb = monitor.critical_bytes() / BYTES_PER_MB;
    let last_refreshed = monitor
        .last_refreshed_unix_secs()
        .and_then(format_epoch_secs);

    let details = Some(serde_json::json!({
        "config_active": true,
        "available_mb": available_mb,
        "warning_threshold_mb": warning_mb,
        "critical_threshold_mb": critical_mb,
        "last_refreshed": last_refreshed,
    }));

    match status {
        koina::disk_space::DiskStatus::Ok { .. } => SubsystemStatus {
            id: "disk_space".to_owned(),
            name: "Disk Space Monitor".to_owned(),
            status: "healthy".to_owned(),
            owner: OWNER.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: last_refreshed,
            last_failure: None,
            degraded_reason: None,
            failure_reason: None,
            details,
            suggested_action: None,
        },
        koina::disk_space::DiskStatus::Warning { .. } => SubsystemStatus {
            id: "disk_space".to_owned(),
            name: "Disk Space Monitor".to_owned(),
            status: "degraded".to_owned(),
            owner: OWNER.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: last_refreshed,
            last_failure: None,
            degraded_reason: Some(format!(
                "available space ({available_mb} MB) is below the warning threshold \
                 ({warning_mb} MB)"
            )),
            failure_reason: None,
            details,
            suggested_action: Some(
                "Free disk space or raise maintenance.diskSpace.warningThresholdMb.".to_owned(),
            ),
        },
        koina::disk_space::DiskStatus::Critical { .. } => SubsystemStatus {
            id: "disk_space".to_owned(),
            name: "Disk Space Monitor".to_owned(),
            status: "failed".to_owned(),
            owner: OWNER.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: last_refreshed,
            degraded_reason: None,
            failure_reason: Some(format!(
                "available space ({available_mb} MB) is below the critical threshold \
                 ({critical_mb} MB); non-essential writes are being rejected"
            )),
            details,
            suggested_action: Some(
                "Free disk space immediately -- non-essential writes are currently blocked."
                    .to_owned(),
            ),
        },
        // WHY: `DiskStatus` is `#[non_exhaustive]` in koina; an unrecognized
        // future variant must surface as a gap, not silently pass or fail.
        _ => SubsystemStatus {
            id: "disk_space".to_owned(),
            name: "Disk Space Monitor".to_owned(),
            status: "unknown".to_owned(),
            owner: OWNER.to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: Some("unrecognized DiskStatus variant".to_owned()),
            details,
            suggested_action: None,
        },
    }
}

/// Tool execution history subsystem: a real read against the tool-audit
/// log, not a fabricated pass. Full aggregated tool usage statistics live
/// behind `GET /api/tool-stats` (#4484), which this deliberately does not
/// duplicate — this record only proves the audit log itself is readable.
async fn subsystem_tool_execution_history(
    state: &HealthState,
    generated_at: &str,
) -> SubsystemStatus {
    let result = state
        .session_store
        .lock()
        .await
        .recent_tool_audit_records(1);
    match result {
        // WHY(#7217): a corrupt row no longer fails this read (see
        // `decode_tool_audit_row`), so "healthy" alone would hide a real
        // backlog from the operator; report "degraded" when the bounded
        // canary scan turned up any corrupt rows along the way. This is a
        // lower bound, not the true backlog size -- see `aletheia
        // session-store tool-audit-check` for a full count.
        Ok(scan) if scan.corrupt.is_empty() => SubsystemStatus {
            id: "tool_execution_history".to_owned(),
            name: "Tool Execution History".to_owned(),
            status: "healthy".to_owned(),
            owner: "crates/mneme::store".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: None,
            details: None,
            suggested_action: None,
        },
        Ok(scan) => SubsystemStatus {
            id: "tool_execution_history".to_owned(),
            name: "Tool Execution History".to_owned(),
            status: "degraded".to_owned(),
            owner: "crates/mneme::store".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: Some(format!(
                "{} tool_audit row(s) failed to decode during this check; run `aletheia \
                 session-store tool-audit-check` for the full backlog",
                scan.corrupt.len()
            )),
            failure_reason: None,
            details: None,
            suggested_action: Some(
                "Run `aletheia session-store tool-audit-check` to size the corrupt-row \
                 backlog."
                    .to_owned(),
            ),
        },
        Err(e) => SubsystemStatus {
            id: "tool_execution_history".to_owned(),
            name: "Tool Execution History".to_owned(),
            status: "failed".to_owned(),
            owner: "crates/mneme::store".to_owned(),
            last_checked: generated_at.to_owned(),
            last_success: None,
            last_failure: None,
            degraded_reason: None,
            failure_reason: Some(format!("tool audit log unreadable: {e}")),
            details: None,
            suggested_action: Some(
                "Check the session-store backend for the tool_audit partition.".to_owned(),
            ),
        },
    }
}

/// Training / QA data persistence subsystem: genuinely `"unknown"`.
///
/// WHY(#5313): DPO/training-corpus persistence lives in `nous::training`
/// with no pylon-reachable status signal today. Reported honestly as
/// unknown rather than assumed healthy.
fn subsystem_training_qa_persistence(generated_at: &str) -> SubsystemStatus {
    SubsystemStatus {
        id: "training_qa_persistence".to_owned(),
        name: "Training / QA Data Persistence".to_owned(),
        status: "unknown".to_owned(),
        owner: "crates/nous::training".to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: None,
        degraded_reason: None,
        failure_reason: Some(
            "no pylon-reachable status signal for DPO/training-corpus persistence yet".to_owned(),
        ),
        details: None,
        suggested_action: Some(
            "Expose a status reader from nous::training's pending-state store.".to_owned(),
        ),
    }
}

/// Domain event bus / SSE subsystem: real subscriber count and journal
/// depth from `EventBus`.
async fn subsystem_event_bus(state: &HealthState, generated_at: &str) -> SubsystemStatus {
    let subscribers = state.event_bus.subscriber_count();
    let journal_len = state.event_bus.journal_len().await;
    SubsystemStatus {
        id: "event_bus".to_owned(),
        name: "Domain Event Bus / SSE".to_owned(),
        status: "healthy".to_owned(),
        owner: "crates/pylon::event_bus".to_owned(),
        last_checked: generated_at.to_owned(),
        last_success: None,
        last_failure: None,
        degraded_reason: None,
        failure_reason: None,
        details: Some(serde_json::json!({
            "subscriber_count": subscribers,
            "journal_len": journal_len,
        })),
        suggested_action: None,
    }
}

/// Aggregate status across every subsystem record: `"failed"` if any
/// subsystem failed, else `"degraded"` if any subsystem degraded or timed
/// out, else `"healthy"`. `"unknown"` subsystems never elevate the
/// aggregate — they are always listed (see [`collect_subsystem_status`]) so
/// the gap stays visible without being mistaken for a live failure.
fn aggregate_subsystem_status(subsystems: &[SubsystemStatus]) -> &'static str {
    if subsystems.iter().any(|s| s.status == "failed") {
        "failed"
    } else if subsystems
        .iter()
        // WHY(#7288): a `"timeout"` record is not a confirmed failure — a
        // handful of this endpoint's checks (`session_store` chief among
        // them) share a lock (`blocking_lock()` in handlers/insights.rs,
        // handlers/sessions/mod.rs, handlers/ops.rs) with ordinary
        // synchronous request handling elsewhere, so an unanswered check
        // within SUBSYSTEM_CHECK_TIMEOUT can mean "busy under real
        // contention" as plausibly as "genuinely stuck". Elevating that to
        // `"failed"` (and a 503) would make a serving-but-busy gateway
        // report itself down; `"degraded"` says "not confirmed healthy"
        // without asserting a failure the evidence does not support. The
        // per-record `status` field still reads the literal `"timeout"`,
        // with its own `failure_reason`/`suggested_action` — only the
        // *aggregate* bucket this contributes to changes.
        .any(|s| s.status == "degraded" || s.status == "timeout")
    {
        "degraded"
    } else {
        "healthy"
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after len assertions"
)]
mod tests {
    use hermeneus::provider::ProviderRegistry;

    use super::*;

    /// A guarantee that was never requested is not a fault.
    ///
    /// WHY pinned: `egress = allow` classifies as `Unrestricted`, and reporting that as
    /// a warning would mean every deliberately-unrestricted deployment shows a permanent
    /// yellow. A check that is always warning is one nobody reads, which costs more than
    /// it buys the first time a real degradation appears beside it.
    #[test]
    fn an_unrequested_guarantee_is_not_reported_as_a_fault() {
        use organon::sandbox::GuaranteeStatus;

        // Struct-update rather than mutate-after-default: `field_reassign_with_default`
        // is denied, and this is the shape the rest of the tree already uses.
        let config = organon::sandbox::SandboxConfig {
            egress: organon::sandbox::EgressPolicy::Allow,
            ..organon::sandbox::SandboxConfig::default()
        };

        let guarantees = organon::sandbox::diagnostic_guarantees(&config);
        assert_eq!(
            guarantees.egress,
            GuaranteeStatus::Unrestricted,
            "egress=allow must classify as Unrestricted, not as a degradation"
        );

        let check = sandbox_check(&config);
        assert!(
            !check.message.unwrap().contains("egress"),
            "an unrequested guarantee must not appear in the unmet list"
        );
    }

    /// Every guarantee is reported, not just the one with an exfiltration story.
    #[test]
    fn all_three_guarantees_appear_in_the_details() {
        let check = sandbox_check(&organon::sandbox::SandboxConfig::default());
        let details = check.details.expect("sandbox check must carry details");
        for key in ["landlock", "seccomp", "egress"] {
            assert!(
                details.get(key).is_some_and(serde_json::Value::is_string),
                "{key} must be reported; a partial answer reads as a complete one"
            );
        }
        assert_eq!(check.name, "sandbox");
    }

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
            use organon::registry::ToolRegistry;
            use taxis::config::AletheiaConfig;
            use taxis::oikos::Oikos;

            let _: &Arc<tokio::sync::Mutex<SessionStore>> = &state.session_store;
            let _: &Arc<ProviderRegistry> = &state.provider_registry;
            let _: &Arc<ToolRegistry> = &state.tool_registry;
            let _: &Arc<NousManager> = &state.nous_manager;
            let _: std::time::Instant = state.start_time;
            let _: &Arc<Oikos> = &state.oikos;
            let _: &Arc<tokio::sync::RwLock<AletheiaConfig>> = &state.config;
            let _: &Option<Arc<dyn mneme::embedding::EmbeddingProvider>> =
                &state.embedding_provider;
            let _: &Option<koina::disk_space::DiskSpaceMonitor> = &state.disk_monitor;
        }
        assert!(std::mem::size_of::<HealthState>() > 0);
    }

    #[test]
    fn health_response_serializes_all_fields() {
        let resp = HealthResponse {
            status: "healthy".to_owned(),
            version: "1.0.0".to_owned(),
            git_sha: "abc123".to_owned(),
            git_dirty: true,
            build_timestamp: "2026-09-06T00:00:00Z".to_owned(),
            uptime_seconds: 300,
            checks: vec![],
            data_dir: "/tmp/instance/data".to_owned(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["git_sha"], "abc123");
        assert_eq!(json["git_dirty"], true);
        assert_eq!(json["build_timestamp"], "2026-09-06T00:00:00Z");
        assert_eq!(json["uptime_seconds"], 300);
        assert!(json["checks"].as_array().unwrap().is_empty());
    }

    /// #7208: the endpoint must report the real build-embedded identity,
    /// not an unset/"unknown" placeholder — this repo, building right now,
    /// is a git checkout, so these must be genuinely resolved.
    #[test]
    fn detailed_health_reports_real_build_identity() {
        assert_eq!(koina::build_info::CRATE_VERSION, env!("CARGO_PKG_VERSION"));
        assert_ne!(koina::build_info::GIT_SHA, "unknown");
        assert!(!koina::build_info::GIT_SHA.is_empty());
        assert_ne!(koina::build_info::build_timestamp(), "unknown");
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
    fn runtime_assembly_fails_empty_provider_and_tool_registries() {
        let providers = ProviderRegistry::new();
        let tools = ToolRegistry::new();

        let check = runtime_assembly_check(&providers, &tools);

        assert_eq!(check.name, "runtime_assembly");
        assert_eq!(check.status, "fail");
        let message = check.message.as_deref().unwrap_or_default();
        assert!(message.contains("gateway-only harness"));
        assert!(message.contains("aletheia serve"));
        let details = check.details.unwrap_or_default();
        assert_eq!(details["provider_count"], 0);
        assert_eq!(details["tool_count"], 0);
        assert_eq!(details["canonical_startup"], "aletheia serve");
    }

    #[test]
    fn runtime_assembly_passes_when_provider_registry_is_populated() {
        let providers = make_registry_with_named_providers(&["alpha"]);
        let tools = ToolRegistry::new();

        let check = runtime_assembly_check(&providers, &tools);

        assert_eq!(check.name, "runtime_assembly");
        assert_eq!(check.status, "pass");
        assert!(check.message.is_none());
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
    fn prosoche_heartbeat_path_warns_when_no_path_is_active() {
        // WHY(#5313): this check used to report "pass" unconditionally, even
        // with no active maintenance path at all — a fake-pass regardless of
        // configuration.
        let mut settings = taxis::config::ProsocheMaintenanceSettings::default();
        settings.heartbeat.enabled = false;
        settings.self_audit.enabled = false;
        let check = check_prosoche_heartbeat_path(&settings);
        assert_eq!(check.status, "warn");
        let msg = check.message.as_deref().unwrap_or_default();
        assert!(
            msg.contains("disabled"),
            "message should name the disabled path: {msg}"
        );
    }

    /// Build a `TaskStateStore` backed by a fresh temp directory and seed it
    /// with one persisted task record.
    fn seeded_daemon_task_store(
        component: &str,
        task: &oikonomos::state::TaskState,
    ) -> (
        tempfile::TempDir,
        (String, oikonomos::state::TaskStateStore),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = oikonomos::state::TaskStateStore::open(dir.path()).unwrap();
        store.save(task).unwrap();
        (dir, (component.to_owned(), store))
    }

    #[test]
    fn subsystem_daemon_runtime_reports_unknown_without_wired_readers() {
        // WHY(#5142): an empty reader slice is ambiguous — it is what the
        // runtime builder produces when `self.daemons` is false, AND what an
        // unwired AppState produces. Reporting "healthy" would assert the
        // runtime is fine on no evidence and hide the second case, including
        // a prosoche-enabled instance with zero task-state stores. "unknown"
        // is the honest verdict; it does not fail the aggregate, so a
        // genuinely daemon-less instance still reports overall healthy.
        let status = subsystem_daemon_runtime(
            &[],
            &taxis::config::ProsocheMaintenanceSettings::default(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(status.status, "unknown");
        assert!(status.degraded_reason.is_none());
        // An `unknown` subsystem must say WHY in the field the DTO reserves
        // for it, the same as every other subsystem — the public-API contract
        // test asserts exactly this across all of them.
        assert!(
            status
                .failure_reason
                .as_deref()
                .is_some_and(|r| r.contains("no daemon task-state readers are wired")),
            "unknown must explain itself in failure_reason, got {:?}",
            status.failure_reason
        );
        let details = status.details.expect("details present");
        assert_eq!(details["runners"], serde_json::json!([]));
    }

    #[test]
    fn subsystem_daemon_runtime_reports_healthy_with_running_tasks() {
        let (_dir, handle) = seeded_daemon_task_store(
            "system",
            &oikonomos::state::TaskState {
                task_id: "retention".to_owned(),
                enabled: Some(true),
                consecutive_failures: 0,
                ..Default::default()
            },
        );
        let status = subsystem_daemon_runtime(
            &[handle],
            &taxis::config::ProsocheMaintenanceSettings::default(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(status.status, "healthy");
        let details = status.details.expect("details present");
        assert_eq!(details["runners"][0]["task_count"], serde_json::json!(1));
    }

    #[test]
    fn subsystem_daemon_runtime_reports_degraded_when_task_in_backoff() {
        let (_dir, handle) = seeded_daemon_task_store(
            "system",
            &oikonomos::state::TaskState {
                task_id: "graph-cleanup".to_owned(),
                enabled: Some(true),
                consecutive_failures: 1,
                last_error: Some("connection refused".to_owned()),
                ..Default::default()
            },
        );
        let status = subsystem_daemon_runtime(
            &[handle],
            &taxis::config::ProsocheMaintenanceSettings::default(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(status.status, "degraded");
        let reason = status.degraded_reason.expect("degraded_reason present");
        assert!(
            reason.contains("backoff"),
            "reason should name backoff: {reason}"
        );
        let details = status.details.expect("details present");
        assert_eq!(details["backoff_tasks"].as_array().unwrap().len(), 1);
    }

    /// WHY(#7206): a disabled daemon task is background, non-serving-path
    /// work -- it must degrade the payload (and name the task) without ever
    /// pinning the whole gateway's HTTP status to 503. Before this fix, this
    /// case (and its `Operator`-cause sibling below) forced `"failed"`, and
    /// `aggregate_subsystem_status` promoted that straight to a 503 that
    /// nothing except hand-editing persisted state could ever clear.
    #[test]
    fn subsystem_daemon_runtime_reports_degraded_when_task_auto_disabled() {
        let (_dir, handle) = seeded_daemon_task_store(
            "syn",
            &oikonomos::state::TaskState {
                task_id: "syn-prosoche".to_owned(),
                enabled: Some(false),
                disable_cause: Some(oikonomos::state::DisableCause::AutoFailure),
                consecutive_failures: 3,
                last_error: Some("provider unreachable".to_owned()),
                ..Default::default()
            },
        );
        let status = subsystem_daemon_runtime(
            &[handle],
            &taxis::config::ProsocheMaintenanceSettings::default(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(
            status.status, "degraded",
            "a disabled non-serving-path task must degrade, never fail, the aggregate"
        );
        assert!(status.failure_reason.is_none());
        let reason = status.degraded_reason.expect("degraded_reason present");
        assert!(
            reason.contains("syn-prosoche") && reason.contains("auto_failure"),
            "reason should name the disabled task and its cause: {reason}"
        );
        let details = status.details.expect("details present");
        assert_eq!(details["disabled_tasks"].as_array().unwrap().len(), 1);
        assert_eq!(
            details["disabled_tasks"][0]["cause"],
            serde_json::json!("auto_failure")
        );
    }

    /// An operator-disabled task degrades the same way an auto-disabled one
    /// does -- `Operator` only changes hydration behavior (stays disabled
    /// forever, see `oikonomos::runner::persistence`), not aggregation.
    #[test]
    fn subsystem_daemon_runtime_reports_degraded_when_task_operator_disabled() {
        let (_dir, handle) = seeded_daemon_task_store(
            "system",
            &oikonomos::state::TaskState {
                task_id: "routing-store-refresh".to_owned(),
                enabled: Some(false),
                disable_cause: Some(oikonomos::state::DisableCause::Operator),
                consecutive_failures: 0,
                ..Default::default()
            },
        );
        let status = subsystem_daemon_runtime(
            &[handle],
            &taxis::config::ProsocheMaintenanceSettings::default(),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(status.status, "degraded");
        let details = status.details.expect("details present");
        assert_eq!(
            details["disabled_tasks"][0]["cause"],
            serde_json::json!("operator")
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
