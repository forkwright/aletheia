# ADR-005: Tool approval guard

## Status

Proposed

## Context

aletheia's desktop daily-driver (proskenion) and TUI (koilon) parse `tool_approval_required` and `tool_approval_resolved` stream events and render an approval overlay. The backend, however, emits the *event* without enforcing the *contract*: in `crates/nous/src/execute/dispatch.rs:548-617`, `ToolApprovalRequired` fires for `Required`/`Mandatory` tools, but the very next statement (line 617) sends `ToolStart` and proceeds to execute. There is no await, no inbound channel for the user's decision, and `ToolApprovalResolved` with a real verdict (`approved`/`denied`) is never emitted — only the synthetic `auto_approved` decision for `None`/`Advisory` reversibility classes.

The consequence is an incoherent safety contract for v1.0.0. The frontend shows an approval dialog that the user cannot actually answer in time: by the time the overlay renders, the irreversible call has already executed. `exec` is marked `Reversibility::Irreversible` (`crates/organon/src/builtins/workspace.rs:946`); `rm` likewise (`crates/organon/src/builtins/fs_ops.rs:514`); `http_request`, `message`, `sessions_dispatch`, `computer_use`, and `web_fetch` are marked Irreversible. Each of these is a write-once, undo-impossible action on the user's host. Shipping the desktop daily-driver with these in an ungated state would advertise reversibility-based safety the runtime does not enforce.

A parallel hole exists in spawn (`crates/nous/src/spawn_svc.rs:143-146`): when a `sessions_spawn`/`sessions_dispatch` request supplies an unrecognized role and no explicit `allowed_tools`, the spawned actor's `tool_allowlist` resolves to `None`, which the execution-time gate (`crates/nous/src/execute/mod.rs:638-659`) treats as "no allowlist = all tools allowed". Spawned actors with unconstrained tool access defeat the parent's approval guard because the spawn itself runs as a single tool call — the user approves the spawn, not the eight tools the child then runs.

The current `ApprovalRequirement` enum already maps cleanly from `Reversibility`:

```rust
// crates/organon/src/types/mod.rs:318-327
impl From<Reversibility> for ApprovalRequirement {
    fn from(rev: Reversibility) -> Self {
        match rev {
            Reversibility::FullyReversible => Self::None,
            Reversibility::Reversible => Self::Advisory,
            Reversibility::PartiallyReversible => Self::Required,
            Reversibility::Irreversible => Self::Mandatory,
        }
    }
}
```

The frontend wiring (`skene::events::StreamEvent::ToolApprovalRequired` / `skene::events::StreamEvent::ToolApprovalResolved`, `proskenion::api::streaming`, `koilon::update::streaming::StreamToolApprovalRequired`) is already in place and ready to drive an overlay. Pylon's stream DTO (`crates/pylon/src/stream_dto.rs:103-115`) and stream forwarder (`crates/pylon/src/handlers/sessions/streaming.rs:572-589`) already pass these events through. The only missing piece is the backend gate and the return path.

Issue #3958 flagged this as a v1.0.0 release-readiness blocker. The accepted direction is to implement the real guard rather than strip the metadata.

## Decision

**aletheia gates tool execution on user approval for `Required` and `Mandatory` reversibility classes, with a typed inbound decision channel and a default-deny timeout. Spawned actors with no resolvable role template receive a conservative read-only allowlist rather than unrestricted access.**

### Reversibility class → approval requirement

The existing `Reversibility → ApprovalRequirement` mapping (`crates/organon/src/types/mod.rs:318-327`) is canonical. The guard interprets the requirement as follows:

| Requirement | Reversibility source | Backend behavior |
|---|---|---|
| `None` | `FullyReversible` (read, grep, ls, view_file, git_status, web_search, memory_search, plan_status, …) | Execute immediately. Emit `ToolApprovalResolved { decision: "auto_approved" }` for the frontend to record. |
| `Advisory` | `Reversible` (write, edit, git_checkout, mkdir, cp, mv, note, blackboard, plan_create, sessions_ask) | Execute immediately. Emit `ToolApprovalResolved { decision: "advisory_auto" }`. The frontend may opt to surface these in a post-hoc audit panel but must not block on them. |
| `Required` | `PartiallyReversible` (sessions_spawn, memory_correct, memory_retract, memory_forget, plan_execute, issue_triage, issue_approve, mathesis) | Emit `ToolApprovalRequired`. Block on the approval channel. On `Approved`: emit `ToolApprovalResolved { decision: "approved" }`, proceed. On `Denied`: emit `ToolApprovalResolved { decision: "denied" }`, skip execution, synthesize a denial `ToolResult` for the model. On timeout: `decision: "timeout_denied"`, default-deny. |
| `Mandatory` | `Irreversible` (exec, rm, http_request, message, sessions_send, sessions_dispatch, computer_use, web_fetch, dokimasia, dromeus, epitropos, parateresis, katharos) | Same as `Required`, but with the policy that the decision channel **must** be wired by the caller; an absent channel with a `Mandatory` requirement is treated as immediate denial rather than silent execution. |

### Signal lifecycle

For every tool call in `dispatch_tools_streaming`:

1. Resolve `ApprovalRequirement` from the tool's reversibility metadata.
2. If `None`/`Advisory`: emit `ToolApprovalResolved { decision: "auto_approved" | "advisory_auto" }`, fall through to step 6.
3. If `Required`/`Mandatory`: emit `ToolApprovalRequired { turn_id, tool_id, tool_name, input, risk, reason }`.
4. Await the next `ApprovalDecision` for this `tool_id` on the approval channel, with a configurable timeout (default 120s).
5. Emit `ToolApprovalResolved { tool_id, decision }` where `decision ∈ {"approved", "denied", "timeout_denied"}`. On `denied`/`timeout_denied`, synthesize a `ContentBlock::ToolResult { is_error: true, content: "Tool execution denied by user." }` and continue to the next tool — do not execute.
6. If approved or auto-approved: emit `ToolStart`, execute, emit `ToolResult`.

The current `TurnStreamEvent::ToolApprovalResolved { tool_id, decision }` shape is sufficient; the `decision` string is extended to the closed set above. The frontend already treats this as opaque text (`crates/theatron/skene/src/events`), so this is forward-compatible.

### Decision channel

A new `nous::approval::ApprovalGate` owns a `tokio::sync::mpsc::Receiver<ApprovalDecision>` and a timeout `Duration`. The gate is held by the entity running the turn:

```rust
pub struct ApprovalDecision {
    pub tool_id: String,
    pub choice: ApprovalChoice,
}

pub enum ApprovalChoice { Approved, Denied }

pub struct ApprovalGate { /* rx + timeout */ }

impl ApprovalGate {
    pub async fn await_decision(&mut self, tool_id: &str) -> ApprovalChoice { /* drain stale tool_ids, timeout-default-deny */ }
}
```

`execute_streaming` accepts `approval_gate: Option<&mut ApprovalGate>` and forwards it to `dispatch_tools_streaming`. `None` is permitted only for non-interactive contexts (batch CLI, tests that assert auto-approve paths); the policy in step 4 above turns absent-gate + `Mandatory` into denial, not silent execution.

Pylon owns the sender side. The `POST /api/v1/sessions/stream` handler creates an `(approval_tx, approval_rx)` pair per turn, wraps the `rx` in an `ApprovalGate`, and hands ownership to the nous turn. A companion `POST /api/v1/sessions/{session_id}/approvals` endpoint routes inbound decisions: pylon looks up the session's `approval_tx` in a `ApprovalGateRegistry: DashMap<SessionId, Sender<ApprovalDecision>>` and sends the decoded `ApprovalDecision`. The registry entry is removed when the turn completes (the `Sender` drops; the gate's `Receiver` sees channel-closed as denial).

### Spawned-actor allowlist default

`crates/nous/src/spawn_svc.rs:143-146` is amended so that the fallback when neither `request.allowed_tools` nor a role template applies is **not** `None`. Instead, a `CONSERVATIVE_SPAWN_ALLOWLIST` constant (`["read", "grep", "find", "ls", "view_file", "memory_search"]`) is applied. A spawned actor with an unknown role or absent template receives a read-only safe set, never unrestricted access.

The top-level desktop nous (`crates/aletheia/src/runtime/nous_config.rs:134`) retains `tool_allowlist: None` because the *user* — through the approval gate — is the gatekeeper at that layer. The spawned-actor layer has no user to gate, so the policy must be enforced statically.

### Configuration knobs

A single `taxis::config::ApprovalConfig` carries the timeout and a `auto_approve_advisory: bool` flag (default `true`). The conservative spawn allowlist is a code-level constant for v1.0.0 — exposing it as configurable is deferred until a real user surfaces a need.

## Consequences

**Positive:**

- **Honest safety contract.** What the frontend advertises (an approval gate) is now what the backend enforces. v1.0.0 can ship.
- **Default-deny on timeout.** A user who never sees the overlay (background tab, dropped connection) gets denial, not execution. The cost of a missed approval is a recoverable retry; the cost of a missed denial is an irreversible action on the host.
- **Spawned actors lose the silent-bypass.** A child agent can no longer escape the parent's approval gate by being granted unrestricted tools through a fallback path.
- **Channel-based design composes.** The `ApprovalGate` does not assume HTTP — koilon (TUI) can drive it directly from its overlay handler without going through pylon.

**Negative:**

- **API surface change to `execute_streaming`.** Callers gain an `Option<&mut ApprovalGate>` parameter. All existing callers (pylon streaming handler, koilon, integration tests) require updates in the same PR.
- **Latency on Required/Mandatory tools.** Each gated call adds at least one round-trip to the user's UI. For a daily-driver this is the correct trade; for batch contexts callers should construct a NousConfig with a tool_allowlist that excludes Mandatory tools rather than relying on auto-approve.
- **Conservative spawn allowlist may surprise.** Existing flows that spawn ad-hoc roles and relied on full tool access will now see denial blocks. The role templates (coder/researcher/reviewer/explorer/runner) already carry explicit allowlists, so first-party usage is unaffected. Third-party callers must pass `request.allowed_tools` explicitly.
- **Decision channel registry adds shared state to pylon.** The `DashMap<SessionId, Sender>` is small, but its lifecycle (insert on stream start, remove on stream end, panic-safe drop on cancel) is one more thing to get right. Mitigated by binding the sender's lifetime to the streaming handler's scope.

## References

- forkwright/aletheia#3958 (this work)
- forkwright/aletheia#3958 (v1.0.0 release-readiness)
- Accepted direction (2026-05-29): implement the real guard, do not strip the metadata
- `crates/organon/src/types/mod.rs:252-327` (Reversibility, ApprovalRequirement, From impl)
- `crates/nous/src/execute/dispatch.rs:548-617` (current emission and the missing gate)
- `crates/nous/src/spawn_svc.rs:143-146` (spawn allowlist fallback)
- `crates/nous/src/stream.rs:13-41` (TurnStreamEvent)
- `crates/pylon/src/stream_dto.rs:101-115` (DTO mirror)
- `crates/pylon/src/handlers/sessions/streaming.rs:572-589` (forwarder)
- ADR-002 (energeia orchestrator shape) — the spawn lifecycle that this ADR hardens
- ADR-004 (pylon middleware ordering) — the request stack that the approval endpoint joins
