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

### Why this order

1. **Turn Safety** — Correctness. Messages silently vanish. Nothing else matters if the pipeline drops turns.
2. **Webchat UX** — Daily usability. SSE endpoint fixes staleness, refresh resilience stops killing agent work, notifications enable multi-agent workflow. Depends on reliable pipeline (1).
3. **Auth & Updates** — Security + maintainability. Login replaces insecure token, update CLI eliminates manual deploys, GitHub Releases enable versioning. Depends on stable UI (2).
4. **Cost-Aware Orchestration** — Economic sustainability. Sub-agents on cheaper models cut Anthropic spend 40-60%. Message queue and plan mode improve interaction model. Depends on reliable pipeline (1) and working UI (2).
5. **Plug-and-Play Onboarding** — Adoption. Setup wizard, process management, agent CLI. The capstone — depends on everything else being stable and well-designed. Ship last.

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
