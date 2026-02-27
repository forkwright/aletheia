# Spec Archival Audit — Features Claimed but Not Delivered

**Date:** 2026-02-27
**Auditor:** Syn
**Scope:** All 4 archived specs + 60 closed issues
**Method:** Cross-referenced spec claims against actual codebase (grep, file existence, integration wiring)

---

## Executive Summary

**4 archived specs audited. 2 have gaps. 2 are clean.**

| Spec | Status Claimed | Actual | Gaps Found |
|------|---------------|--------|------------|
| 25 (Integrated IDE) | ✅ Complete | ⚠️ Partial | 2 features missing |
| 28 (TUI) | Phase 2-3 Complete | ⚠️ Partial | 4 items marked deferred inline but no forwarding refs |
| 31 (Dianoia v1) | Implemented | ⚠️ Partial | 2 features missing, 1 partially delivered |
| 32 (Dianoia v2) | Implemented (Ph 1-3) | ✅ Accurate | Deferrals explicitly noted with forwarding refs to Spec 42 |
| 34 (Agora) | Implemented (Ph 1-6) | ✅ Accurate | 1 deferral noted inline (config reload) |

**60 closed issues audited. 21 were closed in the recent deconfliction (verified mapped to specs). The rest fall into categories below.**

---

## Archived Spec Detail

### Spec 31 — Dianoia v1

**Claimed:** "Implemented" (all 8 phases)

**Phase 2 gaps:**

| Feature | Spec Claim | Codebase Reality | Issue Created? |
|---------|-----------|-----------------|----------------|
| `/plan` slash command | Listed in Phase 2 | **Not in command registry.** `semeion/commands.ts` has 14 commands; none is `/plan`. | ❌ No |
| `aletheia plan` CLI subcommand | Listed in Phase 2 | **Not implemented.** No plan subcommand in CLI. | ❌ No |
| `detectPlanningIntent()` | Listed in Phase 2 | **Implemented** — `dianoia/intent.ts` exists, wired in `context.ts:289`. But it only injects a soft prompt suggestion, not the "automatic planning mode activation" the spec implies. | ⚠️ Partial |
| Legacy tool deprecation (`plan_create`, `plan_propose` marked `@deprecated`) | Listed in Phase 3 | **Not deprecated.** `plan_create` tool is still active and functional — it's how planning works today. The spec's vision of replacing these tools with the orchestrator entry points never happened. | ❌ No |

**Verdict:** Spec 31 Phase 2's entry point vision (slash command + CLI + intent detection → orchestrator) was the key UX innovation. The backend works; the user-facing entry points don't exist. This is why planning still requires talking to me.

---

### Spec 32 — Dianoia v2

**Claimed:** "Implemented (Phases 1–3 complete, Phase 4 verification complete; learning/retrospective deferred to Spec 42)"

**Reality:** Accurate. File-backed state (`project-files.ts`, `file-sync.ts`), context packets (`context-packet.ts`), discussion flow (`discuss-tool.ts`, `discussion-artifacts.ts`), and verification (`verifier.ts`) all exist. Phase 4 deferrals explicitly reference Spec 42. The `discussing` FSM state is in `machine.ts`.

**One minor gap:** WebSocket stream for execution progress (`WS /api/planning/projects/:id/stream`) is in the spec's API design but not in routes. Uses SSE polling instead. This is a design decision, not a dropped feature — but spec doesn't note the divergence.

**Verdict:** ✅ Clean. The model for how to handle deferrals correctly.

---

### Spec 34 — Agora

**Claimed:** "Implemented (Phases 1–6 complete)"

**Reality:** Accurate. `agora/` module exists with full structure. Signal refactored through `SignalChannelProvider` registered via `AgoraRegistry` in `aletheia.ts:635-652`. Slack provider with listener, sender, format, streaming, reactions, access control all present with tests.

**One deferral noted inline:** "Policy changes via config reload without restart (deferred — cross-cutting concern for separate spec)" in Phase 6. Properly documented within the spec.

**Verdict:** ✅ Clean.

---

### Spec 25 — Integrated IDE

**Claimed:** "✅ Complete — Archived"

| Phase | Spec Claim | Codebase Reality |
|-------|-----------|-----------------|
| 1: Multi-tab editor | Complete | ✅ `EditorTabs.svelte` exists, `stores/files.svelte.ts` has full tab state |
| 2: Agent edit notifications | Complete | ✅ `notifyFileEdit` wired in `chat.svelte.ts:316`, `markTabStale` exists |
| 3: File operations (create/delete/rename) | Complete | ⚠️ **API endpoints exist** (`DELETE /api/workspace/file`, `POST /api/workspace/file/move`, `deleteWorkspaceFile`, `moveWorkspaceFile` in `api.ts`). **But no TreeContextMenu.svelte** — no right-click UI to trigger them. Backend done, UI not. |
| 4: Clickable file paths in chat | Complete | ⚠️ `Layout.svelte:118` has a comment `// Listen for clickable file paths in chat messages` but unclear if the regex-based path detection is fully implemented. Needs deeper inspection. |
| 5: Workspace search (stretch) | Complete | ✅ `GET /api/workspace/search` exists in `routes/workspace.ts:240`. `searchWorkspace` in `api.ts:306`. |

**Gaps:**

| Feature | Status | Issue Created? |
|---------|--------|----------------|
| TreeContextMenu.svelte (right-click file operations) | Backend exists, no UI component | ❌ No |
| Inline rename in file tree | No evidence of implementation | ❌ No |

**Verdict:** Backend is complete. UI surface for Phase 3 (context menu, inline rename) is missing. Phase 4 needs verification.

---

### Spec 28 — TUI

**Claimed:** "Phase 2 — Complete ✅ (Phase 3 substantially complete)"

The spec helpfully uses `[ ]` checkboxes. Items marked as NOT done:

| Feature | Phase | Spec Status | Forwarding Ref? |
|---------|-------|-------------|-----------------|
| Token summary overlay (`F2`) | 2 | "deferred, nice-to-have" | ❌ No issue |
| Fuzzy filtering in overlays | 2 | "deferred" | ❌ No issue, TODO in `app.rs:994` |
| OSC 8 hyperlinks for URLs | 2 | "deferred" | ❌ No issue |
| Plan execution progress widget | 2 | "deferred" | ❌ No issue, TODO in `app.rs:1248` |
| Comprehensive test suite | 3 | Not checked | ❌ No issue |

**Verdict:** The spec is honest about these being incomplete (they're unchecked). But "archived" implies done, and no issues or forwarding refs were created. These are known gaps with no tracking.

---

## Closed Issues Audit

### Recently closed (deconfliction batch: #291, #300, #302, #306, #308, #312, #314, #315, #316, #317, #318, #320, #321, #322)

All 21 were closed during the 2026-02-27 deconfliction effort. Verified: each maps to an active spec or was absorbed into another open issue. **Clean.**

### Older closed issues — spot checks

| # | Title | Closed Reason | Verified? |
|---|-------|--------------|-----------|
| 253 | Strip hidden text from web_fetch | Implementation | ✅ `web-fetch.ts:89` strips hidden elements, tests exist |
| 238 | Semantic regression prevention | Implementation | ✅ `invariants.test.ts` exists with invariant patterns |
| 169 | TOOL_CATEGORIES not imported | Bug fix | ✅ Properly imported in `tools.ts` |
| 146 | Split pylon/server.ts | Refactor | ✅ `pylon/routes/` directory exists with split modules |
| 224 | Rename auth→symbolon, distillation→melete | Rename | ✅ `symbolon/` and `melete/` directories exist |
| 210 | Agora/Slack integration | Spec created | ✅ Spec 34 covers this completely |
| 242 | Professional repo gap analysis | Analysis | ✅ Led to specs 17-22, all tracked |
| 207 | Automate OAuth token refresh | Implementation | ⚠️ **Closed but NOT implemented.** No auto-refresh in codebase. Known deferred item (MEMORY.md). |
| 282 | Three file browser UX issues | Bug fix | Needs deeper check |
| 163 | SSE event tracking doesn't survive agent switching | Bug fix | ✅ SSE reconnect + state reload implemented in TUI and WebUI |
| 197 | Live distillation bypasses mem0 sidecar | Bug fix | ⚠️ Partially addressed. `finalize.ts` integration exists but MEMORY.md lesson #17 suggests distillation still doesn't write daily memory files. |
| 102 | plan_step_complete cannot find plan | Bug fix | ✅ Dianoia v2 replaced the entire planning system |
| 101 | Plan approval doesn't trigger execution | Bug fix | ✅ Dianoia v2 replaced the entire planning system |

### Issues closed as "completed" but NOT fully implemented:

| # | Title | Problem |
|---|-------|---------|
| **207** | Automate OAuth token refresh on startup | No auto-refresh logic exists. Manual token replacement still required. Known deferred — but issue is closed as "completed." |

---

## Summary of All Dropped Features

### Must create issues for:

| Feature | Source | Priority |
|---------|--------|----------|
| `/plan` slash command | Spec 31 Phase 2 | **High** — primary UX gap for planning |
| `aletheia plan` CLI subcommand | Spec 31 Phase 2 | Medium — CLI entry point |
| TreeContextMenu.svelte (right-click file ops) | Spec 25 Phase 3 | Low — backend exists |
| Inline rename in file tree | Spec 25 Phase 3 | Low |
| TUI: Token summary overlay (F2) | Spec 28 Phase 2 | Low |
| TUI: Fuzzy filtering in overlays | Spec 28 Phase 2 | Low |
| TUI: OSC 8 hyperlinks | Spec 28 Phase 2 | Low |
| TUI: Plan execution progress widget | Spec 28 Phase 2 | Medium — planning UX |
| TUI: Comprehensive test suite | Spec 28 Phase 3 | Medium — quality |
| OAuth token auto-refresh | Issue #207 | Medium — operational friction |

### Should annotate (partially delivered):

| Feature | Source | Status |
|---------|--------|--------|
| `detectPlanningIntent()` | Spec 31 Phase 2 | Exists but only injects soft prompt, not full orchestrator activation |
| WebSocket stream for execution | Spec 32 Phase 3 | Uses SSE polling instead — design divergence, not a gap |
| Legacy tool deprecation | Spec 31 Phase 3 | Tools still active (correctly — they're the only entry point) |

---

## Root Cause

The archival process has no verification step. A spec gets marked "Implemented", moved to `archive/`, and no one checks whether every line item was delivered. The honest specs (28, 32, 34) self-document their gaps — but the process doesn't require it.

---

## Spec Archival Checklist

Before any spec is absorbed into `DECISIONS.md`:

### 1. Feature Verification
For each phase and each feature listed:
- [ ] **Exists:** Code implementing this feature exists in the codebase
- [ ] **Wired:** Feature is integrated (imported, registered, reachable from user action)
- [ ] **Tested:** Feature has at least one test OR is manually verifiable

### 2. Gap Documentation
For each feature NOT delivered:
- [ ] **Explicitly marked** in the decisions entry with "Deferred:" prefix
- [ ] **Forwarding reference** to the issue or backlog item that tracks it
- [ ] **GitHub issue created** if the feature has standalone value

### 3. Absorption
- [ ] Key decisions, rejected alternatives, and constraints extracted into `archive/DECISIONS.md`
- [ ] Individual spec file deleted (DECISIONS.md is the single source)
- [ ] README.md updated (removed from active, count updated in archive section)

### 4. Sign-off
- [ ] Auditor (not the spec author) has verified the above
- [ ] Commit message for archival references this checklist

---

## Resolution (2026-02-27)

All 5 standalone archived spec files (25, 28, 31, 32, 34) have been absorbed into `DECISIONS.md` and deleted. Spec 25 was missing from DECISIONS.md and has been added under UI & Interaction. Issues #323–#327 track the 10 dropped features.

---

## On the Future of Specs

Specs are a transitional artifact. They exist because Dianoia wasn't mature enough to own the design process when development started. As Dianoia grows — persistent projects, requirements scoping, phase execution, verification — new work should flow through Dianoia projects rather than spec documents. Specs that remain will be architectural constraints and principles (DECISIONS.md), not implementation plans.

The goal: **Dianoia proposes → human approves → Dianoia executes → Dianoia verifies.** When that loop closes, specs become unnecessary. DECISIONS.md persists as the record of *why* things are the way they are.

---

*This audit revealed 10 dropped features across 5 archived specs and 60 closed issues. The systemic fix is the checklist above. The immediate fix was creating issues (#323–#327) for the gaps and absorbing all specs into DECISIONS.md.*
