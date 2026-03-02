# Specifications

> **📋 The master plan lives at [`docs/PROJECT.md`](../PROJECT.md).** All specs, issues, and
> ideas have been consolidated there. This directory holds the detail docs that
> PROJECT.md references — individual specs are subordinate to the project plan.

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

> **Specs are transitional.** As Dianoia matures, new work should flow through
> Dianoia projects (propose → approve → execute → verify) rather than spec
> documents. Specs that remain become architectural constraints and principles
> ([DECISIONS.md](archive/DECISIONS.md)), not implementation plans.

## Active Specs

### The Plan — Rewrite + Instance Structure

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 43 | [Rust Rewrite](43_rust-rewrite.md) | Planned | Single binary, 14 crates, merged sidecar, Tokio actors |
| 44 | [Oikos](44_oikos.md) | Draft | Instance structure, 3-tier hierarchy (theke/shared/nous), cascading resolution |

### Retained — Independent Concerns

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 29 | [UI Layout & Theming](29_ui-layout-and-theming.md) | In Progress | Svelte UI — survives rewrite unchanged |
| 30 | [Homepage Dashboard](30_homepage-dashboard.md) | Absorbed → Spec 45 | Task board and activity feed become workspace views |
| 40 | [Testing Strategy](40_testing-strategy.md) | Draft | Adapt for Rust crates (vitest→cargo test) |
| 41 | [Observability](41_observability.md) | Draft | tracing crate, metrics, spans |
| 43b | [A2UI Live Canvas](43_a2ui-canvas.md) | Integrated → Spec 45 | Canvas surfaces render within the workspace |
| 45 | [Coworking Workspace](45_coworking-workspace.md) | Draft | Shared operations surface — human + agent coworking, controls, health, cost, replay |

### Deferred — Post-Rewrite

| # | Spec | Status | Notes |
|---|------|--------|-------|
| 22 | [Interop & Workflows](22_interop-and-workflows.md) | Deferred | A2A protocol, workflow engine — needs stable platform |
| 24 | [Aletheia Linux](24_aletheia-linux.md) | Deferred | eBPF/DBus collectors + NixOS module — needs stable binary |

## Absorbed into Rewrite (2026-02-28)

These specs' key decisions are preserved in Spec 43, Spec 44, and [DECISIONS.md](archive/DECISIONS.md). The individual spec files remain for reference but are no longer active design targets.

| # | Spec | Absorbed Into | Key Ideas Preserved |
|---|------|--------------|-------------------|
| 27 | [Embedding Space Intelligence](27_embedding-space-intelligence.md) | mneme crate | Voyage-4 migration, embedding-space ops, semantic turn bypass |
| 33 | [Gnomon Alignment](33_gnomon-alignment.md) | Rust crate boundaries + oikos migration | Crate naming = gnomon. TELOS/MNEME renames in migration. Barrel exports moot (Rust enforces) |
| 35 | [Context Engineering](35_context-engineering.md) | nous + taxis crates | Cache-group bootstrap, skill relevance, turn bypass classifier |
| 36 | [Config Taxis](36_config-taxis.md) | Spec 44 (Oikos) | 4-layer → 3-tier oikos. SecretRef retained in taxis crate |
| 37 | [Metadata Architecture](37_metadata-architecture.md) | Spec 44 principles | Declarative cascade, convention-based discovery |
| 38 | [Provider Adapters](38_provider-adapters.md) | hermeneus crate | `trait LlmProvider` with multi-provider support |
| 39 | [Autonomy Gradient](39_autonomy-gradient.md) | dianoia crate | Confidence-gated execution, configurable via oikos cascade |
| 42 | [Nous Team](42_nous-team.md) | nous + daemon crates | Competence routing, reflection loops, automatic MNEME promotion |

## Implemented (Archived)

33 implemented specs (01–25, 26, 28, 31, 32, 34) consolidated into **[archive/DECISIONS.md](archive/DECISIONS.md)**. Organized by domain, preserving key decisions, rejected alternatives, and patterns that constrain future work.

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by creation order)
- **Status:** Draft → In Progress → Implemented → Archived → Absorbed
- Specs describe *intent and design*, not implementation. Code is the source of truth.
- Implemented specs are absorbed into `archive/DECISIONS.md` — individual spec files are deleted.
- **Pre-archival:** Run the [Archival Checklist](ARCHIVAL-AUDIT.md) to verify all features were delivered.
- **Next available number: 46**

## Issue ↔ Spec Cross-Reference

| Issue | Title | Disposition | Notes |
|-------|-------|------------|-------|
| #352 | Rust rewrite tracking | Active | Meta-issue for Spec 43 |
| #338 | Exec tool quality | Absorbed → Spec 44 | cwd via oikos, timeouts via cascade |
| #332 | OS integration | Retained → Spec 24 | Post-rewrite (eBPF/DBus/NixOS) |
| #328 | Planning dashboard | Retained → Spec 29 | UI bug + redesign |
| #326 | TUI deferred items | Retained | Incremental improvements |
| #319 | A2UI live canvas | Retained → Spec 43b | Post-rewrite feature |
| #349 | Evaluate Rust rewrite | **Closed** | Decision made |
| #343 | deploy.sh broken | **Closed** | No more shell scripts |
| #342 | Shell injection | **Closed** | No more shell scripts |
| #340 | Sidecar security | **Closed** | No more sidecar |
| #339 | Deploy pipeline | **Closed** | No more Node.js |

## Related

- **Open issues:** `gh issue list` — 6 remaining, all either active tracking or post-rewrite
- Spec-worthy issues get promoted to spec files; the issue is closed with a link
