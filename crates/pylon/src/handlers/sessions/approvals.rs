//! Tool approval decision and reconciliation endpoints (#3958, ADR-005; #7207).
//!
//! `POST /api/v1/sessions/{session_id}/approvals` and
//! `POST /api/v1/turns/{turn_id}/tools/{tool_id}/{approve,deny}` route the
//! operator's decision into the nous-side approval gate. The streaming handler
//! registers each pending approval by turn and tool id; session id is context
//! for the session-scoped route, not the lookup key.
//!
//! `GET /api/v1/sessions/{session_id}/approvals` and
//! `GET /api/v1/approvals?nous_id=…` (#7207) are the read half of that same
//! model: a client that connects late, restarts, or reconnects after missing
//! the live `tool.approval_required` event can list what is still pending
//! instead of depending on having seen it. Both reads use the same
//! `ApprovalRegistry` the write route routes decisions through — pending
//! approvals are held in memory only, never persisted, so a pylon restart
//! clears them exactly as it already clears the registry's senders.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use nous::approval::{ApprovalChoice, ApprovalDecision};
use serde::{Deserialize, Serialize};
use symbolon::types::Role;
use tracing::{info, instrument};
use utoipa::ToSchema;

use crate::approval_registry::{PendingApproval, RouteOutcome};
use crate::error::{
    ApiError, ApprovalGoneSnafu, ApprovalNotFoundSnafu, ErrorResponse, FieldError,
    ValidationFailedSnafu,
};
use crate::extract::{Claims, require_nous_access, require_read_role, require_role};
use crate::handlers::sessions::find_session;
use crate::state::SessionsState;

/// Operator decision payload for a pending tool approval.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ApprovalRequest {
    /// The `turn_id` from the matching `message_start` or `tool_approval_required` event.
    pub turn_id: String,
    /// The `tool_use_id` from the matching `tool_approval_required` event.
    pub tool_id: String,
    /// `"approved"` or `"denied"`.
    pub decision: String,
}

/// Acknowledgement of a routed decision.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApprovalResponse {
    /// The decision routed to the active turn: `"approved"` or `"denied"`.
    ///
    /// WHY(#6822): replaces the vestigial `routed: bool` — both failure
    /// branches early-return before the response is built, so every 200
    /// carried `routed: true` and the field could never be observed varying.
    pub decision: String,
}

/// Map a non-routed registry outcome onto the error naming the state the
/// operator is actually in (#6822): 410 with `details.reason` when the
/// approval existed but is gone, 404 when it never did.
fn require_routed(outcome: RouteOutcome, turn_id: &str, tool_id: &str) -> Result<(), ApiError> {
    match outcome {
        RouteOutcome::Routed => Ok(()),
        RouteOutcome::Gone(disposition) => ApprovalGoneSnafu {
            turn_id,
            tool_id,
            disposition,
        }
        .fail(),
        RouteOutcome::Unknown => ApprovalNotFoundSnafu { turn_id, tool_id }.fail(),
    }
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
        (status = 404, description = "Session not found, or no approval was ever registered for the turn/tool pair", body = ErrorResponse),
        (status = 410, description = "Approval is gone: details.reason is already_resolved (details.decision names the winning choice), timed_out, or turn_ended", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
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

    let outcome = state
        .approval_registry
        .try_send(
            Some(&session_id),
            &body.turn_id,
            &body.tool_id,
            ApprovalDecision {
                tool_id: body.tool_id.clone(),
                choice,
            },
        )
        .await;
    require_routed(outcome, &body.turn_id, &body.tool_id)?;

    info!(
        session_id = session_id.as_str(),
        turn_id = body.turn_id.as_str(),
        tool_id = body.tool_id.as_str(),
        decision = body.decision.as_str(),
        "approval decision routed"
    );
    Ok((
        StatusCode::OK,
        Json(ApprovalResponse {
            decision: choice.as_wire_str().to_owned(),
        }),
    ))
}

/// One pending tool approval on the wire (#7207): everything a client needs
/// to rebuild the `tool.approval_required` signal without having seen it
/// live. `session_id` is included even on the session-scoped route so both
/// this and the nous-scoped route below share one shape.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingApprovalDto {
    /// Session that owns the blocked turn.
    pub session_id: String,
    /// Turn the tool call belongs to, per #4853's canonical turn identity.
    pub turn_id: String,
    /// Identifier of the blocked tool call.
    pub tool_id: String,
    /// Display name of the tool awaiting approval.
    pub tool_name: String,
    /// Declared effect scope (e.g. `"critical"`), mirroring the live
    /// `tool_approval_required` stream event's `risk` field.
    pub risk: String,
    /// When the approval was registered, RFC 3339.
    pub requested_at: String,
    /// When the approval gate's own timeout will default-deny this call
    /// absent a decision, RFC 3339.
    pub deadline: String,
}

impl From<PendingApproval> for PendingApprovalDto {
    fn from(approval: PendingApproval) -> Self {
        Self {
            session_id: approval.session_id,
            turn_id: approval.turn_id,
            tool_id: approval.tool_id,
            tool_name: approval.tool_name,
            risk: approval.risk,
            requested_at: approval.requested_at.to_string(),
            deadline: approval.deadline.to_string(),
        }
    }
}

/// Response for both pending-approval reconciliation reads (#7207).
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingApprovalsResponse {
    /// Pending approvals, oldest first.
    pub approvals: Vec<PendingApprovalDto>,
}

/// `GET /api/v1/sessions/{session_id}/approvals` — list pending tool
/// approvals for a session (#7207).
///
/// Ownership-verified exactly like [`resolve`]: same session lookup, same
/// `require_nous_access` scoping. Unlike the write route this needs no
/// `Role::Operator` check — but per #7200/#7227's convergence of every
/// other session-scoped `GET` in this module (`list_sessions`,
/// `get_session`, `history`, `replay`) onto one floor, it does need
/// `Role::Agent`: `Role::Readonly` is documented as dashboard-only and
/// cannot read session content, and pending-approval state is exactly that.
///
/// # Cancel safety
///
/// Cancel-safe. Read-only registry snapshot.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{session_id}/approvals",
    params(
        ("session_id" = String, Path, description = "Session id from the streaming turn"),
    ),
    responses(
        (status = 200, description = "Pending approvals for the session, oldest first", body = PendingApprovalsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Session not found", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn list_session_pending(
    State(state): State<SessionsState>,
    claims: Claims,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    // SECURITY(#7200, #7207): Readonly is dashboard-only; pending-approval
    // state is session content, Agent-or-above, scoped to the caller's own
    // nous_id below -- matches the floor #7227 applied to this module's
    // other reads (list_sessions, get_session, history, replay).
    require_read_role(&claims, Role::Agent)?;
    let session = find_session(&state, &session_id).await?;
    require_nous_access(&claims, &session.nous_id)?;

    let approvals = state
        .approval_registry
        .pending_for_session(&session_id)
        .await;
    Ok((
        StatusCode::OK,
        Json(PendingApprovalsResponse {
            approvals: approvals.into_iter().map(Into::into).collect(),
        }),
    ))
}

/// Query parameters for the nous-scoped pending-approval listing (#7207).
#[derive(Debug, Deserialize)]
pub struct PendingApprovalsQuery {
    /// Agent to list pending approvals for, across every session it owns.
    /// Required: an unscoped caller that wants a single agent's view must
    /// say which one, and a scoped caller's token already implies it (a
    /// mismatched value here is rejected, not silently overridden).
    #[serde(default)]
    pub nous_id: Option<String>,
}

/// `GET /api/v1/approvals?nous_id=…` — list pending tool approvals across
/// every session belonging to one agent (#7207).
///
/// The scoped-token shape: a caller holding only a nous-scoped token has no
/// way to enumerate its own session ids first, so this lists by agent
/// instead of by session. `nous_id` must be supplied and, for a scoped
/// token, must match the token's own scope. Requires `Role::Agent` or
/// above, matching `list_sessions`'s floor (#7200/#7227) for the same
/// reason: this is the unscoped-listing shape of the same session content.
///
/// # Cancel safety
///
/// Cancel-safe. Read-only registry snapshot.
#[utoipa::path(
    get,
    path = "/api/v1/approvals",
    params(
        ("nous_id" = String, Query, description = "Agent to list pending approvals for"),
    ),
    responses(
        (status = 200, description = "Pending approvals for the agent, oldest first", body = PendingApprovalsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden, or a scoped token does not own the requested agent", body = ErrorResponse),
        (status = 422, description = "nous_id is required", body = ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
#[instrument(skip(state, claims))]
pub async fn list_nous_pending(
    State(state): State<SessionsState>,
    claims: Claims,
    Query(query): Query<PendingApprovalsQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // SECURITY(#7200, #7207): Readonly is dashboard-only; matches
    // `list_sessions`'s floor for the same unscoped-listing shape.
    require_read_role(&claims, Role::Agent)?;
    let nous_id = match (claims.nous_id.as_deref(), query.nous_id.as_deref()) {
        (Some(scoped), Some(requested)) if scoped != requested => {
            return Err(ApiError::forbidden("access denied for this agent"));
        }
        (Some(scoped), _) => scoped.to_owned(),
        (None, Some(requested)) => requested.to_owned(),
        (None, None) => {
            return ValidationFailedSnafu {
                errors: vec![FieldError {
                    field: "nous_id".to_owned(),
                    code: "required".to_owned(),
                    message: "nous_id is required".to_owned(),
                }],
            }
            .fail();
        }
    };

    let approvals = state.approval_registry.pending_for_nous(&nous_id).await;
    Ok((
        StatusCode::OK,
        Json(PendingApprovalsResponse {
            approvals: approvals.into_iter().map(Into::into).collect(),
        }),
    ))
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
        (status = 404, description = "No approval was ever registered for the turn/tool pair", body = ErrorResponse),
        (status = 410, description = "Approval is gone: details.reason is already_resolved (details.decision names the winning choice), timed_out, or turn_ended", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
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
        (status = 404, description = "No approval was ever registered for the turn/tool pair", body = ErrorResponse),
        (status = 410, description = "Approval is gone: details.reason is already_resolved (details.decision names the winning choice), timed_out, or turn_ended", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
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

    let outcome = state
        .approval_registry
        .try_send(
            None,
            &turn_id,
            &tool_id,
            ApprovalDecision {
                tool_id: tool_id.clone(),
                choice,
            },
        )
        .await;
    require_routed(outcome, &turn_id, &tool_id)?;

    info!(
        turn_id = turn_id.as_str(),
        tool_id = tool_id.as_str(),
        decision = choice.as_wire_str(),
        "approval decision routed"
    );
    Ok((
        StatusCode::OK,
        Json(ApprovalResponse {
            decision: choice.as_wire_str().to_owned(),
        }),
    ))
}
