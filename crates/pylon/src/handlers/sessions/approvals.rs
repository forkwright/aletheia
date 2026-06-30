//! Tool approval decision endpoint (#3958, ADR-005).
//!
//! `POST /api/v1/sessions/{session_id}/approvals` and
//! `POST /api/v1/turns/{turn_id}/tools/{tool_id}/{approve,deny}` route the
//! operator's decision into the nous-side approval gate. The streaming handler
//! registers each pending approval by turn and tool id; session id is context
//! for the session-scoped route, not the lookup key.

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
use crate::extract::{Claims, require_nous_access, require_role};
use crate::handlers::sessions::find_session;
use crate::state::SessionsState;

/// Turn identifier carried by an approval request.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(transparent)]
struct ApprovalTurnId(String);

impl ApprovalTurnId {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Tool-call identifier carried by an approval request.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(transparent)]
struct ApprovalToolId(String);

impl ApprovalToolId {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Operator decision payload for a pending tool approval.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ApprovalRequest {
    /// The `turn_id` from the matching `message_start` or `tool_approval_required` event.
    #[schema(value_type = String)]
    turn_id: ApprovalTurnId,
    /// The `tool_use_id` from the matching `tool_approval_required` event.
    #[schema(value_type = String)]
    tool_id: ApprovalToolId,
    /// `"approved"` or `"denied"`.
    decision: String,
}

/// Acknowledgement of a routed decision.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApprovalResponse {
    /// `true` if the decision was routed to an active turn.
    routed: bool,
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
        (status = 422, description = "Invalid decision value", body = ErrorResponse),
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

    // Validate the operator's decision before any session lookup so that an
    // invalid decision always surfaces as 422 regardless of session state.
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

    // SECURITY(#4584, #5340): Resolve session ownership and enforce nous scope
    // before routing the decision. A scoped operator token must not be able
    // to approve/deny another agent's tool gate by knowing the session id.
    let session = find_session(&state, &session_id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    let turn_id = body.turn_id.as_str();
    let tool_id = body.tool_id.as_str();
    let routed = state
        .approval_registry
        .try_send(
            Some(&session_id),
            turn_id,
            tool_id,
            ApprovalDecision::new(tool_id, choice),
        )
        .await;

    if !routed {
        return SessionNotFoundSnafu { id: session_id }.fail();
    }

    info!(
        session_id = session_id.as_str(),
        turn_id,
        tool_id,
        decision = body.decision.as_str(),
        "approval decision routed"
    );
    Ok((StatusCode::OK, Json(ApprovalResponse { routed })))
}

/// `POST /api/v1/turns/{turn_id}/tools/{tool_id}/approve` — approve a pending tool.
///
/// # Cancel safety
///
/// Cancel-safe. Stateless lookup-and-send.
#[utoipa::path(
    post,
    path = "/api/v1/turns/{turn_id}/tools/{tool_id}/approve",
    params(
        ("turn_id" = String, Path, description = "Turn id from the streaming turn"),
        ("tool_id" = String, Path, description = "Tool use id from the approval request"),
    ),
    responses(
        (status = 200, description = "Decision routed", body = ApprovalResponse),
        (status = 404, description = "No active approval for turn/tool", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn approve_tool(
    State(state): State<SessionsState>,
    claims: Claims,
    Path((turn_id, tool_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    resolve_path_decision(state, claims, turn_id, tool_id, ApprovalChoice::Approved).await
}

/// `POST /api/v1/turns/{turn_id}/tools/{tool_id}/deny` — deny a pending tool.
///
/// # Cancel safety
///
/// Cancel-safe. Stateless lookup-and-send.
#[utoipa::path(
    post,
    path = "/api/v1/turns/{turn_id}/tools/{tool_id}/deny",
    params(
        ("turn_id" = String, Path, description = "Turn id from the streaming turn"),
        ("tool_id" = String, Path, description = "Tool use id from the approval request"),
    ),
    responses(
        (status = 200, description = "Decision routed", body = ApprovalResponse),
        (status = 404, description = "No active approval for turn/tool", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn deny_tool(
    State(state): State<SessionsState>,
    claims: Claims,
    Path((turn_id, tool_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    resolve_path_decision(state, claims, turn_id, tool_id, ApprovalChoice::Denied).await
}

async fn resolve_path_decision(
    state: SessionsState,
    claims: Claims,
    turn_id: String,
    tool_id: String,
    choice: ApprovalChoice,
) -> Result<impl IntoResponse, ApiError> {
    require_role(&claims, Role::Operator)?;
    // SECURITY(#5340): Legacy path-based routes carry no session_id so nous
    // ownership cannot be verified. Scoped tokens must not use this path;
    // unscoped operator tokens retain full access for backward compatibility.
    if claims.nous_id.is_some() {
        return Err(ApiError::forbidden(
            "scoped tokens must use the session-scoped approval route",
        ));
    }

    let routed = state
        .approval_registry
        .try_send(
            None,
            &turn_id,
            &tool_id,
            ApprovalDecision::new(tool_id.clone(), choice),
        )
        .await;

    if !routed {
        return SessionNotFoundSnafu {
            id: format!("{turn_id}/{tool_id}"),
        }
        .fail();
    }

    info!(
        turn_id = turn_id.as_str(),
        tool_id = tool_id.as_str(),
        decision = choice.as_wire_str(),
        "approval decision routed"
    );
    Ok((StatusCode::OK, Json(ApprovalResponse { routed })))
}
