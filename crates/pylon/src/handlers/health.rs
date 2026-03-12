//! Health check endpoint.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;
use utoipa::ToSchema;

use crate::state::AppState;

/// GET /api/health — liveness + readiness check.
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Health status", body = HealthResponse),
        (status = 503, description = "Service unavailable", body = HealthResponse),
    ),
)]
pub async fn check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();

    let mut checks = Vec::new();

    // Check session store connectivity
    let store_ok = state.session_store.lock().await.list_sessions(None).is_ok();
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

    // Check nous actor health
    let actor_health = state.nous_manager.check_health().await;
    let any_dead = actor_health.values().any(|h| !h.alive);
    checks.push(HealthCheck {
        name: "nous_actors",
        status: if actor_health.is_empty() || any_dead {
            "fail"
        } else {
            "pass"
        },
        message: if actor_health.is_empty() {
            Some("no nous actors registered".to_owned())
        } else if any_dead {
            let dead: Vec<_> = actor_health
                .iter()
                .filter(|(_, h)| !h.alive)
                .map(|(id, _)| id.as_str())
                .collect();
            Some(format!("actors not responding: {}", dead.join(", ")))
        } else {
            None
        },
    });

    let status = if checks.iter().any(|c| c.status == "fail") {
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
        Json(HealthResponse {
            status,
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: uptime,
            checks,
        }),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_serializes_all_fields() {
        let resp = HealthResponse {
            status: "healthy",
            version: "1.0.0",
            uptime_seconds: 300,
            checks: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "healthy");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["uptime_seconds"], 300);
        assert!(json["checks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn health_check_pass_omits_message_when_none() {
        let check = HealthCheck {
            name: "session_store",
            status: "pass",
            message: None,
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["name"], "session_store");
        assert_eq!(json["status"], "pass");
        // message is None — serializes as null (no skip annotation)
        assert!(json["message"].is_null());
    }

    #[test]
    fn health_check_fail_includes_message() {
        let check = HealthCheck {
            name: "providers",
            status: "fail",
            message: Some("no LLM providers registered".to_owned()),
        };
        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["status"], "fail");
        assert_eq!(json["message"], "no LLM providers registered");
    }

    #[test]
    fn aggregate_status_unhealthy_when_any_check_fails() {
        let checks = [
            HealthCheck {
                name: "a",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "b",
                status: "fail",
                message: Some("down".to_owned()),
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
                name: "a",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "b",
                status: "warn",
                message: Some("no providers".to_owned()),
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
                name: "session_store",
                status: "pass",
                message: None,
            },
            HealthCheck {
                name: "providers",
                status: "pass",
                message: None,
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
}
