# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

> **Specs are transitional.** As Dianoia matures, new work should flow through
> Dianoia projects (propose → approve → execute → verify) rather than spec
> documents. Specs that remain become architectural constraints and principles
> ([DECISIONS.md](archive/DECISIONS.md)), not implementation plans.

## Active Specs

### In Progress

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 29 | [UI Layout & Theming](29_ui-layout-and-theming.md) | In Progress | Light theme + agent activity indicators done; sidebar→tab bar + settings dedup pending |
| 35 | [Context Engineering](35_context-engineering.md) | In Progress | Cache-group bootstrap + interaction signals wired; skill relevance + turn bypass pending |

### Draft — Architecture

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 33 | [Gnomon Alignment](33_gnomon-alignment.md) | Draft | Module identity and naming infrastructure |
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
| 43 | [A2UI Live Canvas](43_a2ui-canvas.md) | Draft | Agent-writable dynamic UI surface (from #319) |

### Draft — Future

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Draft | A2A protocol, workflow engine |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Skeleton | OS + network integration |
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | Draft | JEPA principles for agents |

## Implemented (Archived)

33 implemented specs (01–25, 26, 28, 31, 32, 34) consolidated into **[archive/DECISIONS.md](archive/DECISIONS.md)**. Organized by domain (Foundation, Turn Pipeline, Memory, Agents, Security, UI, Extensibility, Platform), preserving key decisions, rejected alternatives, and patterns that constrain future work.

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived → Absorbed into DECISIONS.md
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs are absorbed into `archive/DECISIONS.md` — individual spec files are deleted.
- **Pre-archival:** Run the [Archival Checklist](ARCHIVAL-AUDIT.md) to verify all features were delivered.
- **Next available number: 44**

## Issue ↔ Spec Cross-Reference

| Issue | Title | Spec | Relationship |
|-------|-------|------|-------------|
| #319 | A2UI live canvas | **43** | Promoted to spec |
| #326 | TUI deferred items | — | Stays as issue (incremental improvements) |
| #328 | Planning dashboard | **29** Phase 5 | Folded into UI layout spec |
| #332 | OS-layer integration | **24** | Issue content absorbed into skeleton |
| #338 | Exec tool quality | **35** + **36** | Split: truncation/caps → 35, cwd/timeout/glob → 36 |
| #339 | Deploy pipeline | **36** | Absorbed into Config Taxis |
| #250 | Memory recall (BM25) | — | Stays as issue (single implementation phase) |
| #313 | Prosoche activity | — | Partially shipped (PR #336); remainder stays as issue |
| #340 | Sidecar security (bind + auth) | **36** | Deploy/credential wiring in Config Taxis |
| #341 | Sidecar per-request clients | — | Stays as issue (straightforward perf fix) |
| #342 | Shell injection in start.sh | **36** | Deploy script in Config Taxis |
| #343 | deploy.sh broken systemd ref | **36** | Systemd unit + deploy pipeline in Config Taxis |
| #344 | Cron ignores timezone | — | Stays as issue (bug fix, no design) |
| #345 | Sync I/O on event loop | — | Stays as issue (three independent fixes) |
| #346 | SQLite double conn + dynamic imports | — | Stays as issue (low-friction cleanup) |

## Related

- **Open issues:** `gh issue list` — scoped implementation tasks, policy/ops docs
- **Policy/ops issues** are labeled `policy/ops` — not specs, not feature code
- Spec-worthy issues get promoted to spec files; the issue is closed with a link
