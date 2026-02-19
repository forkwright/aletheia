# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

| # | Spec | Status | Summary |
|---|------|--------|---------|
| 2 | [Webchat UX](02_webchat-ux.md) | Draft | SSE events, cross-agent notifications, refresh resilience, file editor with split pane |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Draft | Login page (user/pass + remember me), self-update CLI, GitHub Releases |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | Draft | Sub-agents, message queue, plan mode, model routing, cost visibility |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | One-command setup, unified config, agent management CLI/UI |
| 6 | [Code Quality](06_code-quality.md) | Phase 1 done | Error handling overhaul, dead code audit — CONTRIBUTING.md + CLAUDE.md in PR #37. Remaining: error sweep, dead code removal, ESLint rules |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | Draft | Performance (fast vector-only recall), utility (search/edit UI, confidence decay), domain-scoped memory |
| 8 | [Memory Continuity](08_memory-continuity.md) | Phase 1 done | Expanded tail (4→10 msgs) in PR #36. Remaining: structured summaries, context editing API, working state, agent notes |
| 9 | [Graph Visualization](09_graph-visualization.md) | Draft | 2D default, progressive loading, named communities, semantic node cards, memory auditing, drift detection |
| 10 | [Thinking UI](10_thinking-ui.md) | Draft | Extended thinking visibility — live summary pill, detail panel, collapsed reasoning on completed messages |

### Priority order

- **2 Webchat UX** — Daily usability. SSE endpoint fixes staleness, refresh resilience stops killing agent work.
- **3 Auth & Updates** — Security. Login replaces insecure token, update CLI eliminates manual deploys.
- **4 Cost-Aware Orchestration** — Economics. Sub-agents on cheaper models cut spend 40-60%.
- **5 Plug-and-Play Onboarding** — Adoption. The capstone — ship last.
- **6 Code Quality** — Parallel. Error sweep + dead code removal can happen alongside anything.
- **7 Knowledge Graph** — Performance. Graph recall too slow. Depends on infrastructure stability (2-3).
- **8 Memory Continuity** — Frontier. Structured summaries, context editing API, working state, agent notes.
- **9 Graph Visualization** — Polish. Depends on knowledge graph backend (7).
- **10 Thinking UI** — UX. Extended thinking pills + detail panel. Can be done alongside 2 (same UI domain).

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Turn Safety](archive/01_turn-safety.md) | PR #38 + #39 | Error propagation, distillation guards, orphan diagnostics, duplicate tool_result fix |
| [Data Privacy](archive/spec-data-privacy.md) | PR #33 | File permissions hardening, retention policy, log sanitization, encrypted export, sidecar auth |
| [Unified Thread Model](archive/spec-unified-thread-model.md) | PR #32 | Transport isolation, thread abstraction, thread summaries, topic branching (all 4 phases) |
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
