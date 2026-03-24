# theatron-tui

Ratatui terminal dashboard for Aletheia: chat, planning, memory, metrics, ops views. 37K lines.

## Read first

1. `src/lib.rs`: Entry point, run_tui, event loop (tokio::select! over terminal, SSE, stream, tick)
2. `src/app/mod.rs`: App struct, DashboardState, ConnectionState, sub-state groups
3. `src/msg.rs`: Msg enum (all application messages), OverlayKind, NotificationKind
4. `src/update.rs`: Message handler (Msg -> state mutations + side effects)
5. `src/state/mod.rs`: State modules (agent, chat, command, editor, filter, input, memory, metrics, ops, overlay)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `App` | `app/mod.rs` | Root state: dashboard, connection, input, render, interaction, layout, config |
| `DashboardState` | `app/mod.rs` | Agents, sessions, messages, costs, context usage |
| `ConnectionState` | `app/mod.rs` | SSE link, stream receiver, reconnect bookkeeping |
| `Msg` | `msg.rs` | All application messages: key events, SSE events, API responses, UI actions |
| `Event` | `events.rs` | Raw event sources: Terminal, Sse, Stream, Tick |
| `ChatMessage` | `state/chat.rs` | Display message with role, content, tool calls, stream phase |
| `InputState` | `state/input.rs` | Emacs-style input: cursor, kill ring, history search, tab completion, image attachments |
| `CommandPaletteState` | `state/command.rs` | Fuzzy-matched command palette with slash suggestions |
| `AgentState` | `state/agent.rs` | Per-agent status, active tools, cost, context usage |
| `FilterState` | `state/filter.rs` | Message/session/agent filtering by scope |
| `Theme` | `theme.rs` | Color scheme with dark/light modes, true color + 256-color depth support |
| `VirtualScroll` | `state/virtual_scroll.rs` | Virtual scrolling for large message lists |
| `MetricsState` | `state/metrics.rs` | Token usage and cost tracking per agent/session |

## Architecture

```
Terminal events ──┐
SSE events ───────┤ tokio::select! ──► map_event() ──► Msg ──► update() ──► state mutation
Stream events ────┤                                                              │
Tick ─────────────┘                                                     view() ◄─┘
```

## Patterns

- **Elm architecture**: Event -> Msg -> update(state) -> view(state). No direct state mutation in views.
- **Dual-stream**: Global SSE for dashboard events + per-message mpsc for turn content. Drained between frames.
- **OSC 8 hyperlinks**: Clickable links emitted post-render as OSC 8 escape sequences.
- **Emacs keybindings**: Kill ring (Ctrl-K/Y/Alt-Y), word movement (Alt-F/B), history search (Ctrl-R).
- **Virtual scroll**: Large message lists use virtual scrolling to maintain 60fps rendering.
- **Frame-rate cap**: 16ms tick interval (~60fps) with missed-tick skip. 33ms minimum render interval.
- **Setup wizard**: First-run TUI wizard for instance initialization (src/wizard.rs).

## Common tasks

| Task | Where |
|------|-------|
| Add message type | `src/msg.rs` (Msg enum) + `src/update.rs` (handler) + `src/mapping.rs` (event mapping) |
| Add view | `src/view/` (new view module) + `src/state/view_stack.rs` (View enum) |
| Add overlay | `src/state/overlay.rs` (overlay type) + `src/view/overlay/` (render) + `src/msg.rs` (OverlayKind) |
| Add keybinding | `src/keybindings.rs` (KeyMap) + `src/mapping.rs` (key -> Msg) |
| Modify theme | `src/theme.rs` (Theme, Colors, dark/light palettes) |
| Add state domain | `src/state/` (new module) + `src/state/mod.rs` (re-export) + `src/app/mod.rs` (field) |

## Dependencies

Uses: theatron-core, koina, ratatui, crossterm, tokio, reqwest, pulldown-cmark, syntect, clap, fuzzy-matcher
Used by: aletheia (binary, feature-gated)
