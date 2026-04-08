# Theatron LOC Audit

> As of v0.13.59 (April 2026). Issue #2317.

## Summary

| Crate | LOC | Files | Tests |
|-------|-----|-------|-------|
| theatron-core | 3,227 | - | 68 |
| theatron-tui | 37,368 | - | 439 |
| theatron-desktop | 45,643 | 162 | 297 |
| **Total** | **86,238** | - | **804** |

theatron accounts for roughly 25% of the aletheia workspace LOC (86K of 348K).

## Largest files

| File | LOC | Purpose |
|------|-----|---------|
| tui/src/app/mod.rs | 834 | TUI application state machine |
| core/src/api/client.rs | 792 | Shared HTTP/SSE API client |
| tui/src/view/chat/mod.rs | 788 | Chat view rendering |
| desktop/src/components/chart.rs | 781 | Chart visualization component |
| tui/src/theme.rs | 762 | TUI color theme definitions |
| desktop/src/state/metrics.rs | 762 | Metrics state management |
| desktop/src/views/ops/credentials.rs | 761 | Credential management view |
| tui/src/update/command.rs | 755 | Command processing |
| tui/src/wizard/state.rs | 728 | Setup wizard state |
| desktop/src/state/memory.rs | 728 | Memory browsing state |

No file exceeds 900 lines. Largest files are view/state modules - expected for UI code.

## Architecture

```
theatron-core  (3K LOC)
    Shared API client, domain types, SSE event stream
    ^
    |
theatron-tui  (37K LOC)              theatron-desktop  (46K LOC)
    Terminal dashboard                    Dioxus WebView app
    ratatui rendering                     447 component functions
    12 view modules                       162 source files
```

Desktop is excluded from the workspace build (GTK/webkit2gtk dependencies). See [DESKTOP.md](DESKTOP.md).

## Observations

- **Desktop is the largest crate in the workspace** (46K LOC) - larger than nous (33K), krites (34K), or any backend crate.
- **Core is lean** (3K LOC) - well-factored shared layer.
- **Test coverage is reasonable** - 804 tests across theatron (10 tests per 1K LOC).
- **No single file is oversized** - all under 900 lines.
- **Desktop component count (447)** is substantial - consider a component inventory document (#2412) for onboarding.
