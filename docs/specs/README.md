# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

| Spec | Status | Summary |
|------|--------|---------|
| [Cost-Aware Orchestration](spec-cost-aware-orchestration.md) | Draft | Sub-agents, message queue, plan mode, model routing, cost visibility |
| [Data Privacy](spec-data-privacy.md) | Draft | Encryption at rest, PII handling, data portability |
| [Plug-and-Play Onboarding](spec-plug-and-play-onboarding.md) | Draft | One-command setup, unified config, agent management CLI/UI |

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Unified Thread Model](archive/spec-unified-thread-model.md) | PR #32 | Transport isolation, thread abstraction, thread summaries, topic branching (all 4 phases) |
| [Auth & Security](archive/spec-auth-and-security.md) | PR #26 + security commits | JWT, RBAC, sessions, audit, TLS, passwords (standalone modules; integration pending) |
| [Modular Runtime Architecture](archive/spec-modular-runtime-architecture.md) | PR #21 | Pipeline decomposition, composable stages |
| [Tool Call Governance](archive/spec-tool-call-governance.md) | PR #22 | Approval gates, timeouts, LoopDetector |
| [Distillation & Memory Persistence](archive/spec-distillation-memory-persistence.md) | Hooks | Workspace flush on distillation |

## Conventions

- **Filename:** `spec-<topic>.md`
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth for how things actually work.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `spec-<topic>.md` in this directory
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
