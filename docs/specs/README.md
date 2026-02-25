# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### Draft

| # | Spec | Status | Scope | Notes |
|---|------|--------|-------|-------|
| 29 | [UI Layout & Theming](29_ui-layout-and-theming.md) | Draft | Layout overhaul, light theme | Sidebar → tab bar, agent status |
| 30 | [Homepage Dashboard](30_homepage-dashboard.md) | Skeleton | Shared task board, overview | Cross-agent visibility |
| 28 | [TUI](28_tui.md) | Draft | Ratatui terminal client | Thin client, SSE streaming, agent switching |
| 25 | [Integrated IDE](25_integrated-ide.md) | Draft | File editor in web UI | Nice-to-have |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | Semantic space analysis, concept drift | Research-grade |
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine | A2A premature per Cody |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration | Long-term vision |

### Priority Order

1. **29** UI Layout & Theming
2. **30** Homepage Dashboard
3. **28** TUI — terminal client
4. **25** Integrated IDE
5. **27** Embedding Space Intelligence
6. **22** Interop & Workflows
7. **24** Aletheia Linux — long-term

## Implemented (Archived)

28 implemented specs consolidated into **[archive/DECISIONS.md](archive/DECISIONS.md)** (~350 lines). Organized by domain (Foundation, Turn Pipeline, Memory, Agents, Security, UI, Extensibility), preserving key decisions, rejected alternatives, and patterns that constrain future work. Code is the source of truth — the archive captures the *why*.

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `NN_<topic>.md` in this directory (next available number: 33)
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
