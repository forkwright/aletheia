# Spec & Issue Disposition

> Every existing spec and issue has been accounted for. This is the definitive triage record.
> Last updated: 2026-03-03

---

## Spec Disposition

### Absorbed Into This Plan

| Spec | Title | Milestone | Status | What It Became |
|------|-------|-----------|--------|----------------|
| 43 | Rust Rewrite | — | Absorbed | MODULE DESIGN NOTES + MILESTONES in PROJECT.md |
| 44 | Oikos | M0 | Active | Instance structure, 3-tier hierarchy, cascading resolution |

### Retained Independently

| Spec | Title | Milestone | Status | Notes |
|------|-------|-----------|--------|-------|
| 22 | Interop & Workflows | M6 | Deferred | A2A, workflow engine, IDE integration — needs stable platform |
| 24 | Aletheia Linux | M6 | Deferred | eBPF/DBus, NixOS module — needs stable binary |
| 29 | UI Layout & Theming | M6 | In Progress | Svelte UI — independent of rewrite |
| 30 | Homepage Dashboard | M6 | Skeleton | Svelte UI — shared task board |
| 40 | Testing Strategy | M5 | Draft | Coverage targets adapted for cargo test |
| 41 | Observability | M5 | Draft | tracing crate, metrics, spans |
| 43b | A2UI Live Canvas | M6 | Draft | Agent-writable UI surface |

### Implemented and Archived

33 specs (01–25, 26, 28, 31, 32, 34) documented in `docs/specs/archive/DECISIONS.md`. Key decisions preserved, code is source of truth.

---

## Issue Disposition

| Issue | Title | Status | Disposition |
|-------|-------|--------|-------------|
| #352 | Rust rewrite tracking | Open | Meta-issue for this plan |
| #338 | Coding tool quality | Open | Absorbed → M2 (organon, per-nous workingDir via oikos) |
| #332 | OS-layer integration | Open | Retained → M6 (Spec 24) |
| #328 | Planning dashboard | Open | Retained → M6 (Spec 29) |
| #326 | TUI deferred items | Open | Retained → M6 |
| #319 | A2UI live canvas | Open | Retained → M6 (Spec 43b) |
| #349 | Evaluate Rust rewrite | Closed | Decision made — this plan |
| #339–346 | Various bugs | Closed | Resolved by rewrite (no Node, no sidecar, no shell scripts) |
| #327 | OAuth auto-refresh | Closed | Built into M1 (hermeneus) |
| #313 | Prosoche signals | Closed | Built into M4 (daemon) |
| #256 | Delivery retry | Closed | Built into M3 (pylon) |
| #239 | Graph maintenance | Closed | Built into M4 (daemon) |
| #250 | Memory recall quality | Closed | Built into M1 (mneme) |

---

## Hackathon Project (Dianoia)

The `proj_c3328a6e7874e4acbfa3bf4f` hackathon project burned down most of its 16 issues. Remaining actionable items (#328, #326, #338) are folded into PROJECT.md milestones. The hackathon project can be closed.
