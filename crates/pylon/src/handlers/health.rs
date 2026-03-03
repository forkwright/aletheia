//! Health check endpoint.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::AppState;

/// GET /api/health — liveness + readiness check.
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

/// Top-level response body for `GET /api/health`.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall health status: `"healthy"`, `"degraded"`, or `"unhealthy"`.
    pub status: &'static str,
    /// Crate version from `CARGO_PKG_VERSION`.
    pub version: &'static str,
    /// Seconds since the server started.
    pub uptime_seconds: u64,
    /// Individual subsystem check results.
    pub checks: Vec<HealthCheck>,
}

/// Result of a single subsystem health check.
#[derive(Debug, Serialize)]
pub struct HealthCheck {
    /// Subsystem name (e.g., `"session_store"`).
    pub name: &'static str,
    /// Check outcome: `"pass"`, `"warn"`, or `"fail"`.
    pub status: &'static str,
    /// Optional human-readable failure message.
    pub message: Option<String>,
}
