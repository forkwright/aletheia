# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

### In Progress

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 25 | [Integrated IDE](archive/25_integrated-ide.md) | ✅ Complete | Multi-tab editor, agent edit notifications, file ops, clickable paths, workspace search — PR #307 |
| 29 | [UI Layout & Theming](29_ui-layout-and-theming.md) | In Progress | Light theme + agent activity indicators done; sidebar→tab bar + settings dedup pending |
| 33 | [Gnomon Alignment](33_gnomon-alignment.md) | Draft | Module identity and naming infrastructure |
| 35 | [Context Engineering](35_context-engineering.md) | In Progress | Cache-group bootstrap + interaction signals wired; skill relevance + turn bypass pending |

### Draft — Architecture

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 36 | [Config Taxis](36_config-taxis.md) | Draft | 4-layer workspace + SecretRef credentials |
| 37 | [Metadata Architecture](37_metadata-architecture.md) | Draft | Declarative config-first design |
| 38 | [Provider Adapters](38_provider-adapters.md) | Draft | Multi-provider hermeneus interface |
| 42 | [Nous Team](42_nous-team.md) | Draft | Closing feedback loops for autonomous operation |

### Draft — Execution & Quality

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 39 | [Autonomy Gradient](39_autonomy-gradient.md) | Draft | Confidence-gated dianoia step execution |
| 40 | [Testing Strategy](40_testing-strategy.md) | Draft | Coverage targets, integration patterns |
| 41 | [Observability](41_observability.md) | Draft | Logs, metrics, traces, alerts |

### Draft — UI & Platform

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 30 | [Homepage Dashboard](30_homepage-dashboard.md) | Skeleton | Shared task board, overview |

### Draft — Future

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | JEPA principles for agents |

## Implemented (Archived)

32 implemented specs (01–23, 26, 28, 31, 32, 34) consolidated into **[archive/DECISIONS.md](archive/DECISIONS.md)**. Organized by domain (Foundation, Turn Pipeline, Memory, Agents, Security, UI, Extensibility, Platform), preserving key decisions, rejected alternatives, and patterns that constrain future work.

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs move to `archive/` with a note on which PR delivered them.
- **Next available number: 43**

## Related

- **Open issues:** `gh issue list` — scoped implementation tasks, policy/ops docs
- **Policy/ops issues** are labeled `policy/ops` — not specs, not feature code
- Spec-worthy issues get promoted to spec files; the issue is closed with a link
