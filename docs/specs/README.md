# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs (Implementation Order)

| # | Spec | Status | Summary |
|---|------|--------|---------|
| 1 | [Turn Safety](01_turn-safety.md) | Draft | Error propagation, distillation guards, orphan prevention — fix silent message failures |
| 2 | [Webchat UX](02_webchat-ux.md) | Draft | SSE events, cross-agent notifications, refresh resilience, file editor with split pane |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Draft | Login page (user/pass + remember me), self-update CLI, GitHub Releases |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | Draft | Sub-agents, message queue, plan mode, model routing, cost visibility |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | One-command setup, unified config, agent management CLI/UI |
| 6 | [Code Quality](06_code-quality.md) | Draft | Error handling overhaul, dead code audit, coding standards (CONTRIBUTING.md) |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | Draft | Performance (fast vector-only recall), utility (search/edit UI, confidence decay), domain-scoped memory |
| 8 | [Memory Continuity](08_memory-continuity.md) | Draft | Survive distillation: working state, structured summaries, context editing API, expanded tail, agent notes |

### Why this order

1. **Turn Safety** — Correctness. Messages silently vanish. Nothing else matters if the pipeline drops turns.
2. **Webchat UX** — Daily usability. SSE endpoint fixes staleness, refresh resilience stops killing agent work, notifications enable multi-agent workflow. Depends on reliable pipeline (1).
3. **Auth & Updates** — Security + maintainability. Login replaces insecure token, update CLI eliminates manual deploys, GitHub Releases enable versioning. Depends on stable UI (2).
4. **Cost-Aware Orchestration** — Economic sustainability. Sub-agents on cheaper models cut Anthropic spend 40-60%. Message queue and plan mode improve interaction model. Depends on reliable pipeline (1) and working UI (2).
5. **Plug-and-Play Onboarding** — Adoption. Setup wizard, process management, agent CLI. The capstone — depends on everything else being stable and well-designed. Ship last.
6. **Code Quality** — Sustainability. Can be done in parallel with anything. Error handling overhaul makes debugging easier for all other specs. Dead code audit reduces surface area. Standards prevent new debt.
7. **Knowledge Graph** — Performance and utility. Graph recall is currently too slow and inconsistent. Fixing this improves every agent's quality. Depends on infrastructure stability (1-3).
8. **Memory Continuity** — The hardest problem. Requires stable distillation (1), working graph (7), and reliable pipeline. Introduces Anthropic's context editing API, working state maintenance, and agent notes. This is the frontier.

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
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
