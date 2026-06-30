---
scope: "crates/theatron/proskenion/"
defers_to: ["../../../CLAUDE.md"]
tightens: ["desktop UI route, state, service, and component guidance"]
---

# proskenion

## At a glance

Dioxus desktop application for Aletheia. Depends on skene. Entry point: `src/main.rs` (proskenion::run).

## Depth

Dioxus desktop application for Aletheia: chat, planning, memory, metrics, ops, and file views. 45K lines.

## Read first

1. `src/app.rs`: Route enum (all views), root component with connection gating
2. `src/state/app.rs`: AppState, TabBar, Overlay (top-level reactive state)
3. `src/state/events.rs`: EventState, SseConnectionState, StreamingState (SSE/stream state)
4. `src/services/mod.rs`: Background services (SSE, streaming, cache, file watcher, keybindings)
5. `src/components/mod.rs`: Shared UI components (chat, charts, markdown, code blocks, graphs)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `AppState` | `state/app.rs` | Top-level state: agents, sessions, tab bar, overlays, connection |
| `EventState` | `state/events.rs` | SSE connection state, streaming state, calls in progress |
| `PylonClient` | `services/connection.rs` | Gateway API client wrapper with auth and reconnection |
| `AgentStore` | `state/agents.rs` | Agent roster with status tracking and selection |
| `ToastStore` | `state/toasts.rs` | Notification toast queue with severity and auto-dismiss |
| `CommandStore` | `state/commands.rs` | Command palette entries and execution |
| `InputState` | `state/input.rs` | Chat input field state with submission tracking |
| `PerMessageStreamState` | `state/streaming.rs` | Per-message streaming accumulator (text, thinking, calls) |
| `ToolCallState` | `state/tools.rs` | Active call tracking with approval state |
| `PlanCardState` | `state/tools.rs` | Plan visualization with step status tracking |
| `WindowState` | `state/platform.rs` | Window geometry persistence across sessions |

## Views

| Route | View | Purpose |
|-------|------|---------|
| `/` | Chat | Streaming chat with markdown, code highlighting, tool calls |
| `/files` | Files | Workspace file tree explorer |
| `/planning` | Planning | Plan creation, requirements, roadmap, project detail |
| `/memory` | Memory | Knowledge graph, fact browser, entity relationships |
| `/metrics` | Metrics | Token usage, cost tracking, tool usage stats, drill-down |
| `/ops` | Ops | System health, credentials, maintenance tasks |
| `/sessions` | Sessions | Session list, archive, rename, export |
| `/meta` | Meta | Agent performance, knowledge growth, system self-reflection |
| `/settings` | Settings | Server list, appearance, keybindings, setup wizard |

## Patterns

- **Signal-based reactivity**: Dioxus signals and stores for all state; background services write into signals.
- **Dual-stream architecture**: Global SSE for dashboard events + per-message stream for turn content.
- **Service coroutines**: Background tasks (SSE, streaming, file watcher) run as Dioxus coroutines or tokio tasks.
- **Platform integration**: Window state persistence and desktop notifications.
- **Workspace excluded**: Not in the cargo workspace due to GTK/webkit2gtk system deps. Inline lints mirror workspace config.

## Common tasks

| Task | Where |
|------|-------|
| Add view/route | `src/app.rs` (Route enum) + `src/views/` (new view module) + `src/layout.rs` (sidebar nav) |
| Add UI component | `src/components/` (new component module) |
| Add state domain | `src/state/` (new state module) + `src/state/mod.rs` (re-export) |
| Add background service | `src/services/` (new service module) |
| Add platform feature | `src/platform/` (window state, notifications) |
| Add overlay | `src/state/app.rs` (Overlay enum) + `src/views/` or `src/components/` (render) |

## Dependencies

Uses: skene, dioxus, tokio, reqwest, pulldown-cmark, syntect, notify-rust
Used by: standalone binary (not in workspace)
