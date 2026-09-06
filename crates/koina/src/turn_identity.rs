//! Canonical turn/request identity, shared across the tool-execution boundary.
//!
//! WHY(#4853): promoted from `nous::stream::TurnEventIdentity` rather than
//! minted again at a lower layer. `organon::ToolContext` needs the same
//! identity nous already builds once per turn, but `organon` sits below
//! `nous` in the crate graph (nous depends on organon, not the reverse), so
//! the struct's canonical home has to be a crate both can see without a
//! cycle. `koina` is that crate: it is the shared foundation every other
//! Aletheia crate depends on and depends on nothing itself. `nous::stream`
//! re-exports this type under its original name so every existing call site
//! (including pylon's `nous::stream::TurnEventIdentity` references) keeps
//! compiling unchanged -- there is still exactly one `TurnEventIdentity`
//! type in the tree, just relocated to where both of its consumers can name
//! it.

use crate::ulid::Ulid;

/// Authoritative identity of the turn emitting a tool-lifecycle event.
///
/// WHY: the approval event previously carried the session-local turn
/// *number* in a field named `turn_id`, and the Pylon bridge silently
/// substituted its own stream ULID -- the same event had two different
/// identities depending on where it was observed. Every tool-lifecycle
/// event now carries the canonical turn ULID (`SessionState::turn_id`), the
/// owning session id, the gateway request id when the turn originated from
/// an HTTP request, and the session-local turn ordinal (#4853).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEventIdentity {
    /// Canonical turn identifier (ULID), stable across actor restarts.
    pub turn_id: Ulid,
    /// Session that owns the turn.
    // kanon:ignore RUST/primitive-for-domain-id WHY: stream events cross a process boundary into pylon DTOs; both sides carry the session id as a plain string
    pub session_id: String,
    /// Canonical HTTP request ID from the gateway, when one exists (#4853).
    pub request_id: Option<String>,
    /// Session-local turn ordinal (`SessionState::turn`), monotonically
    /// increasing within a session (#4853). Distinct from `turn_id`: this is
    /// the position-in-session counter that DPO pair ids, working-memory
    /// checkpoint keys, and hook contexts index by; `turn_id` is the
    /// globally-unique dedup key.
    pub turn_number: u64,
    /// Client-generated turn id, when the originating request supplied one
    /// (#4853). Pylon's `/api/v1/sessions/stream` accepts an optional
    /// `client_turn_id` (a client-minted ULID scoped to one user action) for
    /// idempotent retry; when present it is threaded through here so replay
    /// and audit can recover which client-side action produced this turn.
    /// `None` for turns with no client-supplied id (internal/cross-nous/test
    /// turns, or requests that omitted it).
    pub client_turn_id: Option<String>,
}
