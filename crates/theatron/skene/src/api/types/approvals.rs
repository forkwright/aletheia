//! Pending-approval reconciliation types (#7207).
//!
//! Mirrors pylon's `crate::approval_registry::PendingApproval` /
//! `crate::handlers::sessions::approvals::PendingApprovalDto` -- the read
//! half of the session-scoped approval model `resolve_session_approval`
//! (#7202) writes into.

use serde::{Deserialize, Serialize};

/// One pending tool approval. Mirrors pylon's `PendingApprovalDto`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
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

/// Response for both pending-approval reconciliation reads: `GET
/// /api/v1/sessions/{id}/approvals` and `GET /api/v1/approvals?nous_id=…`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApprovalsResponse {
    /// Pending approvals, oldest first.
    pub approvals: Vec<PendingApproval>,
}
