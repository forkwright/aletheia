//! Health check endpoint.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

/// GET /api/health — liveness + readiness check.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Health status", body = HealthResponse),
    ),
)]
pub async fn check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();

    let mut checks = Vec::new();

    // Check session store connectivity
    let store_ok = state
        .session_store
        .lock()
        .is_ok_and(|store| store.list_sessions(None).is_ok());
    checks.push(HealthCheck {
        name: "session_store",
        status: if store_ok { "pass" } else { "fail" },
        message: if store_ok {
            None
        } else {
            Some("session store unavailable".to_owned())
        },
    });

    // Check provider registry has at least one provider
    let has_providers = !state.provider_registry.providers().is_empty();
    checks.push(HealthCheck {
        name: "providers",
        status: if has_providers { "pass" } else { "warn" },
        message: if has_providers {
            None
        } else {
            Some("no LLM providers registered".to_owned())
        },
    });

    let status = if checks.iter().any(|c| c.status == "fail") {
        "unhealthy"
    } else if checks.iter().any(|c| c.status == "warn") {
        "degraded"
    } else {
        "healthy"
    };

    Json(HealthResponse {
        status,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: uptime,
        checks,
    })
}

/// Top-level health response combining all subsystem checks.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Aggregate status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Crate version from `Cargo.toml`.
    #[schema(value_type = String)]
    pub version: &'static str,
    /// Seconds since server start.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
}

/// Result of a single subsystem health check.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthCheck {
    /// Subsystem name (e.g. `"session_store"`, `"providers"`).
    #[schema(value_type = String)]
    pub name: &'static str,
    /// Check outcome: `"pass"`, `"warn"`, or `"fail"`.
    #[schema(value_type = String)]
    pub status: &'static str,
    /// Diagnostic message when status is not `"pass"`.
    pub message: Option<String>,
}
