# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### Near-Complete (1 phase remaining)

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 16 | [Efficiency](16_efficiency.md) | 5/6 phases | Hot-reload config (5) |

### In Progress

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 20 | [Security Hardening](20_security-hardening.md) | 1/4 phases | Docker sandbox (2), audit trail (3), encrypted memory (4) |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | ~1.5/5 phases | Plan mode, model routing |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Phases 1b, 2a done | Release workflow (1a,1c), auth wiring (2b-2e), UI (3a-3c), failover (4a) |
| 9 | [Graph Visualization](09_graph-visualization.md) | 3/8+ phases | Communities, search, editing |

### Draft

| # | Spec | Status | Scope |
|---|------|--------|-------|
| 18 | [Extensibility](18_extensibility.md) | Draft | Hooks, custom commands, plugins, path safety |
| 21 | [Agent Portability](21_agent-portability.md) | Draft | Export/import agent files, scheduled backups, checkpoint time-travel |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine, IDE integration, event bus hardening, pub/sub |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Agent self-construction, CLI scaffolding, onboarding wizard |
| 25 | [Integrated IDE](25_integrated-ide.md) | Draft | File editor in web UI, shared editing |
| 26 | [Recursive Self-Improvement](26_recursive-self-improvement.md) | Draft | Autonomous tool creation, strategy refinement, memory curation |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | Semantic space analysis, concept drift, embedding-level insights |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration |

### Reference

| # | Document | Purpose |
|---|----------|---------|
| 17 | [Unified Gap Analysis](17_unified-gap-analysis.md) | 8-system comparison, 37 features identified, all mapped to specs |

### Priority Order

**Tier 1 — Finish what's started:**
1. **16** Efficiency — one phase left (hot-reload config)

**Tier 2 — Core capabilities:**
2. **20** Security Hardening — sandbox, audit trail, encryption
3. **18** Extensibility — hooks + commands open the platform
4. **21** Agent Portability — backup/export is genuinely missing

**Tier 3 — Platform maturity:**
5. **4** Cost-Aware Orchestration — plan mode, routing
6. **3** Auth & Updates — release workflow, auth wiring
7. **9** Graph Viz — communities, search
8. **22** Interop & Workflows — A2A, workflow engine
9. **25** Integrated IDE — in-browser editing
10. **26** Recursive Self-Improvement — autonomous capability growth
11. **27** Embedding Space Intelligence — semantic analysis
12. **5** Onboarding — capstone, ship last
13. **24** Aletheia Linux — long-term vision

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Knowledge Graph](archive/07_knowledge-graph.md) | PRs #61, #85, #86 | Vector recall, Neo4j degradation, sufficiency gates, entity CRUD, tool memory |
| [Chat Output Quality](archive/11_chat-output-quality.md) | PR #86 | Narration filter, cost badge, GFM checkboxes, rich components |
| [Session Continuity](archive/12_session-continuity.md) | PRs #53, #85 | Expanded tail, structured summaries, working state, agent notes, post-distillation priming |
| [Sub-Agent Workforce](archive/13_sub-agent-workforce.md) | PRs #72, #86 | Role definitions, tool filtering, dispatch reducer, idempotency |
| [Development Workflow](archive/14_development-workflow.md) | PRs #71, #79, #86 | Spec template, branch convention, PR workflow, CI, doctor --fix |
| [UI Interaction Quality](archive/15_ui-interaction-quality.md) | PRs #54, #72, #86 | Thinking persistence, tool input display, categorization, status line |
| [Sleep-Time Compute](archive/19_sleep-time-compute.md) | PR #80 | Nightly reflection, contradiction detection, self-assessment, weekly synthesis |
| [Memory Pipeline](archive/23_memory-pipeline.md) | PR #83 | Extraction wiring, turn facts, entity resolution, recall quality, corpus backfill |
| [Webchat UX](archive/02_webchat-ux.md) | PR #47 | SSE notifications, refresh resilience, tool output fix, file editor |
| [Turn Safety](archive/01_turn-safety.md) | PR #38 + #39 | Error propagation, distillation guards, orphan diagnostics |
| [Data Privacy](archive/spec-data-privacy.md) | PR #33 | File permissions, retention policy, log sanitization, encrypted export |
| [Code Quality](archive/06_code-quality.md) | PRs #37, #45, #52, #60, #62 | Error taxonomy, dead code audit, oxlint enforcement |
| [Memory Continuity](archive/08_memory-continuity.md) | PRs #36, #43, #44, #55 | Expanded tail, structured summaries, context editing API |
| [Thinking UI](archive/10_thinking-ui.md) | PRs #40, #54, #63 | Extended thinking for Opus, status pills, collapsed reasoning |
| [Auth & Security](archive/spec-auth-and-security.md) | PR #26 | JWT, RBAC, sessions, audit, TLS (standalone; integration pending) |
| [Modular Runtime](archive/spec-modular-runtime-architecture.md) | PR #21 | Pipeline decomposition, composable stages |
| [Tool Call Governance](archive/spec-tool-call-governance.md) | PR #22 | Approval gates, timeouts, LoopDetector |
| [Distillation Persistence](archive/spec-distillation-memory-persistence.md) | Hooks | Workspace flush on distillation |

**Score: 18 archived, 5 in progress, 8 draft/skeleton.**

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `NN_<topic>.md` in this directory (next available number: 28)
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
