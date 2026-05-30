//! Tool approval decision endpoint (#3958, ADR-005).
//!
//! `POST /api/v1/sessions/{session_id}/approvals` routes the operator's
//! decision (from the proskenion overlay or koilon TUI) into the nous-side
//! approval gate. The gate is registered by the streaming handler at turn
//! start; this handler is a thin pass-through that looks up the session
//! sender and forwards the decision.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use nous::approval::{ApprovalChoice, ApprovalDecision};
use serde::{Deserialize, Serialize};
use symbolon::types::Role;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::error::{
    ApiError, ErrorResponse, FieldError, SessionNotFoundSnafu, ValidationFailedSnafu,
};
use crate::extract::{Claims, require_role};
use crate::state::SessionsState;

/// Operator decision payload for a pending tool approval.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ApprovalRequest {
    /// The `tool_use_id` from the matching `tool_approval_required` event.
    pub tool_id: String,
    /// `"approved"` or `"denied"`.
    pub decision: String,
}

/// Acknowledgement of a routed decision.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApprovalResponse {
    /// `true` if the decision was routed to an active turn.
    pub routed: bool,
}

/// `POST /api/v1/sessions/{session_id}/approvals` — resolve a pending tool approval.
///
/// # Cancel safety
///
/// Cancel-safe. Stateless lookup-and-send.
#[utoipa::path(
    post,
    path = "/api/v1/sessions/{session_id}/approvals",
    request_body = ApprovalRequest,
    params(
        ("session_id" = String, Path, description = "Session id from the streaming turn"),
    ),
    responses(
        (status = 200, description = "Decision routed", body = ApprovalResponse),
        (status = 400, description = "Bad decision value", body = ErrorResponse),
        (status = 404, description = "No active turn for session", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims, body))]
pub async fn resolve(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(session_id): Path<String>,
    Json(body): Json<ApprovalRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;

    let choice = match body.decision.as_str() {
        "approved" => ApprovalChoice::Approved,
        "denied" => ApprovalChoice::Denied,
        other => {
            return ValidationFailedSnafu {
                errors: vec![FieldError {
                    field: "decision".to_owned(),
                    code: "invalid_value".to_owned(),
                    message: format!("expected 'approved' or 'denied', got '{other}'"),
                }],
            }
            .fail();
        }
    };

    let routed = state
        .approval_registry
        .try_send(
            &session_id,
            ApprovalDecision {
                tool_id: body.tool_id.clone(),
                choice,
            },
        )
        .await;

    if !routed {
        return SessionNotFoundSnafu { id: session_id }.fail();
    }

    info!(
        session_id = session_id.as_str(),
        tool_id = body.tool_id.as_str(),
        decision = body.decision.as_str(),
        "approval decision routed"
    );
    Ok((StatusCode::OK, Json(ApprovalResponse { routed })))
}
