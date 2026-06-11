//! Ops/registry introspection endpoints.

use axum::Json;
use axum::extract::State;
use symbolon::types::Role;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::OpsState;

#[path = "ops_dto.rs"]
mod ops_dto;
pub use ops_dto::{ActiveTool, OpsToolsResponse, ToolHistoryEntry};

fn metrics_snapshot() -> (u64, u64) {
    let registry = koina::metrics::MetricsRegistry::new();
    registry.with_registry(organon::metrics::register);

    let mut encoded = String::new();
    if let Err(err) = registry.encode(&mut encoded) {
        unreachable!("encoding into a String is infallible: {err}");
    }

    let mut total_calls = 0_u64;
    let mut total_errors = 0_u64;

    for line in encoded.lines() {
        if !line.starts_with("aletheia_tool_invocations_total{") {
            continue;
        }
        let Some((_, value_text)) = line.rsplit_once(' ') else {
            continue;
        };
        let value = value_text.parse::<u64>().unwrap_or(0);
        total_calls = total_calls.saturating_add(value);
        if line.contains("status=\"error\"") {
            total_errors = total_errors.saturating_add(value);
        }
    }

    (total_calls, total_errors)
}

/// GET /api/v1/ops/tools: summarize the live tool registry and metrics.
///
/// The registry listing is sourced from organon's live tool registry. Tool
/// call totals are read from the organon metrics families registered into a
/// scratch registry at request time. The runtime does not currently persist a
/// chronological tool-call history, so `tool_history` is empty until such a
/// store is added.
#[utoipa::path(
    get,
    path = "/api/v1/ops/tools",
    responses(
        (status = 200, description = "Ops tool registry summary", body = OpsToolsResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn tools(
    State(state): State<OpsState>,
    claims: Claims,
) -> Result<Json<OpsToolsResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let active_tools = state
        .tool_registry
        .definitions()
        .into_iter()
        .map(|def| ActiveTool {
            name: def.name.as_str().to_owned(),
            id: def.name.as_str().to_owned(),
        })
        .collect();

    let (total_calls, total_errors) = metrics_snapshot();

    Ok(Json(OpsToolsResponse {
        active_tools,
        tool_history: Vec::new(),
        total_calls,
        total_errors,
    }))
}
