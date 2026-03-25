# theatron

Presentation umbrella grouping the three UI crates: core (shared infrastructure), tui (terminal), desktop (Dioxus). 86K lines total.

## Sub-crates

| Crate | Path | Lines | Purpose |
|-------|------|-------|---------|
| `theatron-core` | `core/` | 3K | Shared API client, types, SSE parser, streaming infrastructure |
| `theatron-tui` | `tui/` | 37K | Ratatui terminal dashboard (Elm architecture, feature-gated in workspace) |
| `theatron-desktop` | `desktop/` | 45K | Dioxus desktop app (excluded from workspace, GTK deps) |

## Dependency graph

```
theatron-core          (leaf: koina, reqwest, tokio)
    ^           ^
    |           |
theatron-tui    theatron-desktop
```

Both UI crates depend on `theatron-core` for API client, domain types, and SSE infrastructure. They never depend on each other.

## Shared infrastructure (core)

- **ApiClient**: HTTP client for the pylon REST API (agents, sessions, history, auth, costs, streaming)
- **Domain IDs**: NousId, SessionId, TurnId, ToolId, PlanId newtypes
- **StreamEvent**: Per-turn streaming events (text deltas, tool calls, plan steps)
- **SseEvent / SseStream**: Wire-level SSE parser for reqwest byte streams

## Workspace status

| Crate | In workspace | Reason |
|-------|-------------|--------|
| `theatron-core` | yes | Pure Rust deps |
| `theatron-tui` | yes | Feature-gated (`tui` feature on aletheia binary) |
| `theatron-desktop` | no | GTK/webkit2gtk system deps break CI |

## Where to make changes

| Task | Sub-crate |
|------|-----------|
| Add API endpoint binding | `core` (ApiClient method + request/response types) |
| Add stream event type | `core` (StreamEvent enum + streaming parser) |
| Add domain ID newtype | `core` (id.rs) |
| Add TUI view | `tui` (view module + View enum + state + Msg handler) |
| Add TUI keybinding | `tui` (keybindings.rs + mapping.rs) |
| Add desktop view/route | `desktop` (Route enum + view module + sidebar nav) |
| Add desktop component | `desktop` (components module) |
| Add desktop state domain | `desktop` (state module) |

## Dependencies

Uses: koina, reqwest, tokio, snafu, ratatui, crossterm, dioxus
Used by: aletheia (binary, tui feature-gated), theatron-desktop (standalone binary)
