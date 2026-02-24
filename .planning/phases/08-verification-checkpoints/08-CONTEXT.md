# Phase 8: Verification & Checkpoints - Context

**Gathered:** 2026-02-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Add two interlocking systems on top of the execution engine from Phase 7: (1) a goal-backward verifier that runs after each phase executes and reports met/partially-met/not-met with gap analysis, and (2) a risk-based checkpoint system that gates high-risk decisions during planning and execution. Both systems must behave correctly in interactive and autonomous mode. Phase 9 (Polish & Migration) handles docs and final integration testing.

</domain>

<decisions>
## Implementation Decisions

### Verifier output format
- On pass: Claude decides the format (brief on pass, detailed on fail — context-appropriate)
- On partial/fail: structured gap list — numbered, each entry contains: the failed criterion, what was found vs expected, and a concrete proposed fix
- Verification result stored on `planning_phases` record (not a separate table) — surfaced in conversation and accessible via existing API
- Multiple verification runs per phase overwrite the stored result (latest wins); audit trail through git/spawn records is sufficient

### Checkpoint triggering criteria
- Hard-coded risk categories as the base (destructive operations, external integrations, config changes, schema migrations) plus agent runtime heuristics that can escalate based on project complexity
- Checkpoints declared per-plan in frontmatter (`checkpoint: true`, `risk_level: low | medium | high`) — predictable and reviewable before execution
- 3-tier system:
  - **Low**: auto-approved silently, logged to `planning_checkpoints`
  - **Medium**: notifies user + fires `planning:checkpoint` event, but does not block execution
  - **High**: pauses execution, requires explicit user input to proceed

### Checkpoint user-facing format (blocking)
- Shows: risk type, exact operation about to happen, consequences of approval vs skip, action choices: **approve / modify / skip**
- All auto-approvals (including low-tier) are logged to `planning_checkpoints` with `auto_approved: true` — full audit trail in autonomous mode too

### Gap-closure interaction flow
- On verification failure: show structured gap list, then offer 3 options: **fix now** / **override and advance** / **abandon**
- Fix now: verifier auto-generates gap plans based on its own findings; user sees a summary and confirms before execution
- Override and advance: phase marked complete with `overridden: true` flag; user note is required (not optional) before advancing
- Partial gap acceptance (fix some, accept others): Claude decides granularity based on gap count and types

### Autonomous mode behavior
- True blocking errors only (require human even in autonomous mode): irreversible destructive operations, security/auth failures, states where proceeding would corrupt project data
- High-risk checkpoints do NOT block in autonomous mode — only the true-blocker list above
- Verification gaps in autonomous mode: auto-generate fix plans, execute them, re-verify — one retry cycle; if still failing, surface to user
- Verification can be disabled entirely via `verifier: false` in PlanningConfig (existing config field); when disabled, phases always advance as complete; checkpoints still fire regardless

### Claude's Discretion
- Whether partial gap override is per-gap or all-or-nothing (based on gap count and types)
- Exact heuristics used by the agent to escalate checkpoint risk level at runtime
- Schema for storing verification result on `planning_phases` (column name, JSON structure)
- `planning_checkpoints` table schema details beyond the required fields (risk_type, auto_approved, user_note, decision)

</decisions>

<specifics>
## Specific Ideas

- The gap list format (criterion + found vs expected + proposed fix) should feel like the output from the existing `gsd-verifier` agent — consistent vocabulary across the internal GSD tooling and the Dianoia verifier
- Auto-approvals in autonomous mode should be completely invisible to the user unless they query the audit log — no noise in the conversation stream

</specifics>

<deferred>
## Deferred Ideas

- None — discussion stayed within phase scope

</deferred>

---

*Phase: 08-verification-checkpoints*
*Context gathered: 2026-02-24*
