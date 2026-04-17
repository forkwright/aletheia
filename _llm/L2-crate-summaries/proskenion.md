# proskenion

**Purpose:** Dioxus desktop application for Aletheia: chat, planning, memory, metrics, ops, and file views. Excluded from default workspace build (requires GTK3).

## Key types

| Type | Purpose |
|------|---------|
| `AppState` | Top-level reactive state: agents, sessions, tab bar, overlays, connection |
| `EventState` | SSE connection state, streaming state, tool calls in progress |
| `PylonClient` | Gateway API client wrapper with auth and reconnection |
| `ToolCallState` | Active tool call tracking with approval state |
| `WindowState` | Window geometry persistence across sessions |

## Public API surface

- `proskenion::run` — entry point: initializes Dioxus app and starts desktop window
- `proskenion::state` — `AppState`, `EventState`, reactive state modules
- `proskenion::services` — background services: SSE, streaming, cache, file watcher, keybindings

## When to look here

- When adding a new desktop view, component, or system tray feature
- When modifying streaming accumulation or per-message tool call display
