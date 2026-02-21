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
| 15 | [UI Interaction Quality](15_ui-interaction-quality.md) | 4/6 phases | Status line enhancement (5), stream preview (6) |
| 14 | [Development Workflow](14_development-workflow.md) | 5/7 phases | Versioning + releases (5), doctor --fix (7) |
| 11 | [Chat Output Quality](11_chat-output-quality.md) | 4/5 phases | Rich message components (5) |

### In Progress

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 13 | [Sub-Agent Workforce](13_sub-agent-workforce.md) | 7/11 phases | Spawn depth (8), tool restrictions (9), reducer (10), dedup (11) |
| 16 | [Efficiency](16_efficiency.md) | 4/6 phases | Hot-reload config (5), cache stability audit (6) |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | 5/11 phases | Graph UI (2c), MMR diversity (3c), sufficiency gates (3d), self-editing memory (3e), tool memory (3f), thread-aware recall (3b) |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | ~1.5/5 phases | Plan mode, model routing |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Auth done | Release workflow, update CLI, credential failover (4a) |
| 9 | [Graph Visualization](09_graph-visualization.md) | 3/8+ phases | Communities, search, editing |

### New (from Gap Analysis)

| # | Spec | Status | Scope |
|---|------|--------|-------|
| 18 | [Extensibility](18_extensibility.md) | Draft | Hooks, custom commands, plugins, path safety (F-2, F-12, F-24, F-25, F-27, F-37) |
| 19 | [Sleep-Time Compute](19_sleep-time-compute.md) | Draft | Reflective memory — nightly pattern extraction, contradiction detection, self-assessment (F-11) |
| 20 | [Security Hardening](20_security-hardening.md) | Draft | PII detection, Docker sandbox, hash chain audit, encrypted memory (F-4, F-6, F-7, F-8) |
| 21 | [Agent Portability](21_agent-portability.md) | Draft | Export/import agent files, scheduled backups, checkpoint time-travel (F-15, F-34) |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine, IDE integration, event bus hardening, pub/sub (F-16, F-17, F-20, F-33, F-36) |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Agent self-construction, CLI scaffolding, onboarding wizard (F-26) |

### Gap Analysis Reference

| # | Document | Purpose |
|---|----------|---------|
| 17 | [Unified Gap Analysis](17_unified-gap-analysis.md) | 8-system comparison, 37 features identified, all mapped to specs above |

**Already implemented from gap analysis:** F-5 (LoopDetector), F-9 (composite scoring), F-30 (temporal decay).
**Not infrastructure:** F-29 (parallel validation — skill pattern, not spec).

### Priority Order

**Tier 1 — Finish what's started:**
1. **12** Session Continuity — one phase left
2. **14** Development Workflow — versioning enables everything
3. **15** UI Quality — small phases, high polish
4. **11** Chat Output — rich components

**Tier 2 — Core capabilities:**
5. **19** Sleep-Time Compute — the most interesting new capability. Reflective memory is what makes agents learn.
6. **20** Security Hardening — PII detection first, then sandbox, then the rest
7. **18** Extensibility — hooks + commands open the platform
8. **7** Knowledge Graph — memory quality drives everything
9. **21** Agent Portability — backup/export is genuinely missing

**Tier 3 — Platform maturity:**
10. **16** Efficiency — hot-reload, cache stability
11. **13** Sub-Agent — spawn depth, tool restrictions
12. **4** Cost-Aware Orchestration — plan mode, routing
13. **3** Auth & Updates — release workflow
14. **22** Interop & Workflows — A2A, workflow engine, IDE
15. **9** Graph Viz — communities, search
16. **5** Onboarding — capstone, ship last

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
