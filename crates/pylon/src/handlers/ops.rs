//! Ops/registry introspection endpoints.

use axum::Json;
use axum::extract::State;
use symbolon::types::Role;
use tracing::warn;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::OpsState;

#[path = "ops_dto.rs"]
mod ops_dto;
pub use ops_dto::{LiveInvocationEntry, OpsToolsResponse, ToolCatalogEntry, ToolHistoryEntry};

const RECENT_TOOL_HISTORY_LIMIT: usize = 100;

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

fn history_entry(record: mneme::types::ToolAuditRecord) -> ToolHistoryEntry {
    let receipt_state = if record.receipt.is_some() {
        "present"
    } else {
        "absent"
    }
    .to_owned();

    ToolHistoryEntry {
        id: record.id,
        session_id: record.session_id,
        nous_id: record.nous_id,
        turn_seq: record.turn_seq,
        tool_call_id: record.tool_call_id,
        tool_name: record.tool_name,
        duration_ms: record.duration_ms,
        is_error: record.is_error,
        outcome: record.outcome,
        result: record.result,
        approval: record.approval,
        receipt_state,
        receipt: record.receipt,
        created_at: record.created_at,
    }
}

/// GET /api/v1/ops/tools: summarize the live tool registry and metrics.
///
/// The registry catalog is sourced from organon's live tool registry. Live
/// invocations are tracked by organon's metrics module and removed when the
/// execution guard drops. Totals and errors are read from the organon
/// Prometheus families. Chronological tool-call history is sourced from
/// mneme's bounded recent tool audit records.
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

    let catalog = state
        .tool_registry
        .definitions()
        .into_iter()
        .map(|def| ToolCatalogEntry {
            name: def.name.as_str().to_owned(),
            description: def.description.clone(),
            id: def.name.as_str().to_owned(),
        })
        .collect();

    let live_invocations = organon::metrics::live_invocations()
        .into_iter()
        .map(|inv| LiveInvocationEntry {
            id: inv.id,
            tool_name: inv.tool_name,
            elapsed_ms: u64::try_from(inv.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
        .collect();

    let (total_calls, total_errors) = metrics_snapshot();
    let state_clone = state.clone();
    let history_result = tokio::task::spawn_blocking(move || {
        let store = state_clone.session_store.blocking_lock();
        store
            .recent_tool_audit_records(RECENT_TOOL_HISTORY_LIMIT)
            .map_err(ApiError::from)
    })
    .await;
    let (history, history_unavailable) = match history_result {
        Ok(Ok(records)) => (
            records.into_iter().map(history_entry).collect::<Vec<_>>(),
            false,
        ),
        Ok(Err(err)) => {
            warn!(error = %err, "failed to read tool audit history");
            (Vec::new(), true)
        }
        Err(err) => {
            warn!(error = %err, "tool audit history task failed");
            (Vec::new(), true)
        }
    };

    Ok(Json(OpsToolsResponse {
        catalog,
        live_invocations,
        history,
        total_calls,
        total_errors,
        history_unavailable,
    }))
}
