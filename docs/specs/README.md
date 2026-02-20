# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 3 | [Auth & Updates](03_auth-and-updates.md) | Auth done (2a-2e) | Part 2 untouched: release workflow, `aletheia update` CLI, update check daemon. Also: migrate-auth CLI, session mgmt UI, update API |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | Phase 1 done, Phase 2 partial | Plan mode, automatic model routing per-turn, cost visibility/tracking |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Everything — zero implementation |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | Phase 1a done | Neo4j optional mode, extraction quality, memory confidence/decay, domain scoping, thread-aware recall |
| 9 | [Graph Visualization](09_graph-visualization.md) | Phase 1-3 done | Named communities, semantic node cards, search overhaul, edit capabilities, memory auditing, conversation archaeology, cross-agent visibility, drift detection (10 phases) |
| 11 | [Chat Output Quality](11_chat-output-quality.md) | Phase 1-2 done | Runtime narration suppression filter, rich message components (status cards, diff views, progress checklists) |

### Priority order

- **3 Auth & Updates** — Part 2 (updates) eliminates manual deploys. Session mgmt UI is polish.
- **4 Cost-Aware Orchestration** — Core cost savings (40-60%) still unbuilt. Plan mode + model routing are the big wins.
- **7 Knowledge Graph** — Making Neo4j optional reduces infrastructure burden. Extraction quality improves memory over time.
- **11 Chat Output Quality** — Runtime narration filter is a safety net. Rich components are polish.
- **9 Graph Visualization** — Deep feature work. Depends on knowledge graph backend (7).
- **5 Plug-and-Play Onboarding** — Capstone. Ship last, after everything else is solid.

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
