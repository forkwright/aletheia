# nous

**Purpose:** Agent session pipeline: bootstrap → recall → execute → finalize. Implements the `NousActor` tokio actor model for sequential turn processing.

## Key types

| Type | Purpose |
|------|---------|
| `NousActor` | Tokio actor processing turns sequentially via inbox channel |
| `NousHandle` | Cloneable sender for invoking turns on a NousActor |
| `NousManager` | Spawns actors, monitors health, routes messages across agents |
| `PipelineContext` | Assembled context flowing through pipeline stages |
| `BootstrapAssembler` | Priority-based system prompt packer from workspace cascade |

## Public API surface

- `nous::actor` - `NousActor` run loop, `NousHandle` for external invocation
- `nous::manager` - `NousManager` lifecycle, health polling, restart
- `nous::bootstrap` - `BootstrapAssembler`, system prompt construction
- `nous::pipeline` - `PipelineContext`, `TurnResult`, stage composition

## When to look here

- When modifying the agent turn pipeline (bootstrap, recall, execute, finalize stages)
- When adding cross-nous routing or changing actor lifecycle management
