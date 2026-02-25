# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### Draft

| # | Spec | Status | Scope | Notes |
|---|------|--------|-------|-------|
| 28 | [TUI](28_tui.md) | Draft | Ratatui terminal client | Thin client, SSE streaming, agent switching |
| 25 | [Integrated IDE](25_integrated-ide.md) | Draft | File editor in web UI | Nice-to-have |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | Semantic space analysis, concept drift | Research-grade |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine | A2A premature per Cody |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration | Long-term vision |

### Reference

| # | Document | Purpose |
|---|----------|---------|
| 17 | [Unified Gap Analysis](17_unified-gap-analysis.md) | 8-system comparison, 37 features identified, all mapped to specs |

### Priority Order

**Draft:**
1. **28** TUI — terminal client
2. **25** Integrated IDE
3. **27** Embedding Space Intelligence
4. **22** Interop & Workflows
5. **24** Aletheia Linux — long-term

## Implemented (Archived)

27 specs implemented and consolidated into [archive/DECISIONS.md](archive/DECISIONS.md) — decisions, rejected alternatives, and patterns that constrain future work.

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `NN_<topic>.md` in this directory (next available number: 31)
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
