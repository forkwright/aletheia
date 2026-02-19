# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

| Spec | Status | Summary |
|------|--------|---------|
| [Unified Thread Model](spec-unified-thread-model.md) | Draft | Transport-isolated execution, seamless continuity, thread abstraction |
| [Auth & Security](spec-auth-and-security.md) | In Progress | Authentication, authorization, threat model |
| [Data Privacy](spec-data-privacy.md) | Draft | PII handling, retention, encryption, compliance |

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
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
