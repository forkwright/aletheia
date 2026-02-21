# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### Near-Complete (1-2 phases remaining)

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 12 | [Session Continuity](12_session-continuity.md) | 9.5/10 phases | Post-distillation priming (5 remainder) |
| 14 | [Development Workflow](14_development-workflow.md) | 6/7 phases | Doctor --fix (7) |
| 15 | [UI Interaction Quality](15_ui-interaction-quality.md) | 4/6 phases | Status line enhancement (5), stream preview (6) |
| 11 | [Chat Output Quality](11_chat-output-quality.md) | 4/5 phases | Rich message components (5) |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | 8/11 phases | Graph UI search+edit (2c), sufficiency gates (3d), tool memory (3f) |

### In Progress

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 13 | [Sub-Agent Workforce](13_sub-agent-workforce.md) | 7/11 phases | Spawn depth (8), tool restrictions (9), reducer (10), dedup (11) |
| 16 | [Efficiency](16_efficiency.md) | 4/6 phases | Hot-reload config (5), cache stability audit (6) |
| 20 | [Security Hardening](20_security-hardening.md) | 1/4 phases | Docker sandbox (2), audit trail (3), encrypted memory (4) |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | ~1.5/5 phases | Plan mode, model routing |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Phases 1b, 2a done | Release workflow (1a,1c), auth wiring (2b-2e), UI (3a-3c), failover (4a) |
| 9 | [Graph Visualization](09_graph-visualization.md) | 3/8+ phases | Communities, search, editing |

### Draft

| # | Spec | Status | Scope |
|---|------|--------|-------|
| 18 | [Extensibility](18_extensibility.md) | Draft | Hooks, custom commands, plugins, path safety (F-2, F-12, F-24, F-25, F-27, F-37) |
| 21 | [Agent Portability](21_agent-portability.md) | Draft | Export/import agent files, scheduled backups, checkpoint time-travel (F-15, F-34) |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine, IDE integration, event bus hardening, pub/sub (F-16, F-17, F-20, F-33, F-36) |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Agent self-construction, CLI scaffolding, onboarding wizard (F-26) |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration |

### Gap Analysis Reference

| # | Document | Purpose |
|---|----------|---------|
| 17 | [Unified Gap Analysis](17_unified-gap-analysis.md) | 8-system comparison, 37 features identified, all mapped to specs above |

**Already implemented from gap analysis:** F-5 (LoopDetector), F-9 (composite scoring), F-10 (MMR diversity), F-22 (memory tools), F-30 (temporal decay).
**Not infrastructure:** F-29 (parallel validation — skill pattern, not spec).

### Priority Order

**Tier 1 — Finish what's started:**
1. **12** Session Continuity — one phase left
2. **14** Development Workflow — one phase left
3. **7** Knowledge Graph — three phases left, mostly small
4. **15** UI Quality — small phases, high polish
5. **11** Chat Output — rich components

**Tier 2 — Core capabilities:**
6. **20** Security Hardening — sandbox, audit trail, encryption
7. **18** Extensibility — hooks + commands open the platform
8. **13** Sub-Agent — spawn depth, tool restrictions
9. **21** Agent Portability — backup/export is genuinely missing

**Tier 3 — Platform maturity:**
10. **16** Efficiency — hot-reload, cache stability
11. **4** Cost-Aware Orchestration — plan mode, routing
12. **3** Auth & Updates — release workflow, auth wiring
13. **22** Interop & Workflows — A2A, workflow engine, IDE
14. **9** Graph Viz — communities, search
15. **5** Onboarding — capstone, ship last

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Webchat UX](archive/02_webchat-ux.md) | PR #47 | SSE notifications, refresh resilience, tool output fix, file editor (CodeMirror 6) |
| [Turn Safety](archive/01_turn-safety.md) | PR #38 + #39 | Error propagation, distillation guards, orphan diagnostics, duplicate tool_result fix |
| [Data Privacy](archive/spec-data-privacy.md) | PR #33 | File permissions hardening, retention policy, log sanitization, encrypted export, sidecar auth |
| [Unified Thread Model](archive/spec-unified-thread-model.md) | PR #32 | Transport isolation, thread abstraction, thread summaries, topic branching (all 4 phases) |
| [Code Quality](archive/06_code-quality.md) | PRs #37, #45, #52, #60, #62 | Error taxonomy, dead code audit, CONTRIBUTING.md, error handling sweep, oxlint enforcement |
| [Memory Continuity](archive/08_memory-continuity.md) | PRs #36, #43, #44, #55 | Expanded tail, structured summaries, context editing API, working state, agent notes |
| [Thinking UI](archive/10_thinking-ui.md) | PRs #40, #54, #63 | Extended thinking for Opus, status pills + detail panel, collapsed reasoning in history |
| [Auth & Security](archive/spec-auth-and-security.md) | PR #26 + security commits | JWT, RBAC, sessions, audit, TLS, passwords (standalone modules; integration pending) |
| [Modular Runtime Architecture](archive/spec-modular-runtime-architecture.md) | PR #21 | Pipeline decomposition, composable stages |
| [Tool Call Governance](archive/spec-tool-call-governance.md) | PR #22 | Approval gates, timeouts, LoopDetector |
| [Distillation & Memory Persistence](archive/spec-distillation-memory-persistence.md) | Hooks | Workspace flush on distillation |
| [Sleep-Time Compute](19_sleep-time-compute.md) | PR #80 | Nightly reflection, contradiction detection, self-assessment, weekly synthesis |
| [Memory Pipeline](23_memory-pipeline.md) | Direct commits | Extraction wiring, turn facts, entity resolution, recall quality, corpus backfill, quality tools |

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by implementation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth for how things actually work.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `NN_<topic>.md` in this directory (next available number)
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
