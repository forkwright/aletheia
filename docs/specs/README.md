# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### In Progress

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 3 | [Auth & Updates](03_auth-and-updates.md) | Phases 1b, 2a done; auth wiring + session UI (PR #99) | Release workflow (1a,1c), remaining auth (2c-2e), login UI (3a-3c), failover (4a) |
| 20 | [Security Hardening](20_security-hardening.md) | 2/4 phases (PR #99) | Audit trail (3), encrypted memory (4) |
| 26 | [Recursive Self-Improvement](26_recursive-self-improvement.md) | 3/6 phases (PR #106) | Pipeline self-config (4), code patching (5), evolutionary search (6) |
| 18 | [Extensibility](18_extensibility.md) | 1/6 phases (PR #98) | Custom commands (2), plugin API (3-4), marketplace (5-6) |
| 21 | [Agent Portability](21_agent-portability.md) | 1/4 phases (PR #100) | Import (2), time-travel (3), migration (4) |

### Draft

| # | Spec | Status | Scope | Notes |
|---|------|--------|-------|-------|
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Agent scaffolding, CLI, wizard | Needed for public adoption |
| 25 | [Integrated IDE](25_integrated-ide.md) | Draft | File editor in web UI | Nice-to-have |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | Semantic space analysis, concept drift | Research-grade |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine | A2A premature per Cody |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration | Long-term vision |

### Reference

| # | Document | Purpose |
|---|----------|---------|
| 17 | [Unified Gap Analysis](17_unified-gap-analysis.md) | 8-system comparison, 37 features identified, all mapped to specs |

### Priority Order

**Tier 1 — Active work (phases in progress):**
1. **3** Auth & Updates — login UI, release workflow
2. **20** Security Hardening — audit trail, encryption
3. **26** Recursive Self-Improvement — pipeline self-config, code patching

**Tier 2 — Continue (phase 1 done, more to go):**
4. **18** Extensibility — custom commands, plugin API
5. **21** Agent Portability — import, time-travel

**Tier 3 — Draft:**
6. **5** Onboarding — needed for public adoption
7. **25** Integrated IDE
8. **27** Embedding Space Intelligence
9. **22** Interop & Workflows
10. **24** Aletheia Linux — long-term

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Cost-Aware Orchestration](archive/04_cost-aware-orchestration.md) | PRs #59, #89, #99 | Model routing, token pricing, sub-agent delegation, plan mode, cost visibility UI |
| [Efficiency](archive/16_efficiency.md) | PRs #75, #94 | Parallel tools, token audit, truncation, dynamic thinking, hot-reload config, prompt cache stability |
| [Graph Visualization](archive/09_graph-visualization.md) | PRs #56, #90, #91 | 2D/3D graph, node cards, communities, search, health audit, drift detection, context lookup |
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

**Score: 21 archived, 5 in progress, 5 draft/skeleton.**

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
