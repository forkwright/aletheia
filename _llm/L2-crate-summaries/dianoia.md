# dianoia

**Purpose:** Multi-phase planning state machine with on-disk workspace persistence. Zero workspace dependencies - fully decoupled from the agent pipeline.

## Key types

| Type | Purpose |
|------|---------|
| `Project` | Top-level project: name, mode, state, phases |
| `ProjectState` | Lifecycle: Created → Questioning → Researching → … → Complete |
| `Phase` | Grouping of related plans with lifecycle state and completion tracking |
| `Plan` | Executable plan: dependencies, iteration limits, blockers |
| `ProjectWorkspace` | On-disk persistence: PROJECT.json, phases/, blockers/, artifacts/ |

## Public API surface

- `dianoia::project` - `Project`, `ProjectMode`, lifecycle management
- `dianoia::workspace` - `ProjectWorkspace` for on-disk JSON persistence
- `dianoia::stuck` - `StuckDetector` for pattern-based loop detection

## When to look here

- When extending the planning state machine (new states, transitions, or plan types)
- When modifying project workspace layout or persistence format
