# koilon

**Purpose:** Ratatui terminal dashboard for Aletheia: chat, planning, memory, metrics, and ops views with Elm-style message-driven architecture.

## Key types

| Type | Purpose |
|------|---------|
| `App` | Root state: dashboard, connection, input, render, interaction, layout, config |
| `Msg` | All application messages: key events, SSE events, API responses, UI actions |
| `DashboardState` | Agents, sessions, messages, costs, context usage |
| `InputState` | Emacs-style input: cursor, kill ring, history search, tab completion |
| `Theme` | Color scheme with dark/light modes and true color / 256-color support |

## Public API surface

- `koilon::run_tui` - entry point: takes `ApiClient` + config, runs terminal event loop
- `koilon::app` - `App`, `DashboardState`, `ConnectionState`
- `koilon::msg` - `Msg` enum (all application messages)

## When to look here

- When adding a new TUI view, panel, or keyboard shortcut
- When modifying SSE event handling or connection management in the terminal UI
