# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** - they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Index

| Spec | Status | Summary |
|------|--------|---------|
| [Modular Runtime Architecture](spec-modular-runtime-architecture.md) | Draft | Plugin system, loader, lifecycle, hot-reload |
| [Auth & Security](spec-auth-and-security.md) | Draft | Authentication, authorization, threat model |
| [Data Privacy](spec-data-privacy.md) | Draft | PII handling, retention, encryption, compliance |
| [Tool Call Governance](spec-tool-call-governance.md) | Draft | Approval flows, risk tiers, audit logging |
| [Distillation & Memory Persistence](spec-distillation-memory-persistence.md) | Draft | Context compaction, memory tiers, continuity |

## Conventions

- **Filename:** `spec-<topic>.md`
- **Status:** Draft → Review → Accepted → Superseded
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth for how things actually work.

## Adding a Spec

1. Create `spec-<topic>.md` in this directory
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
