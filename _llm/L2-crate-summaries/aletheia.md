# aletheia

**Purpose:** Binary entrypoint: Clap CLI, server startup, and service wiring that connects all crates into the running Aletheia runtime.

## Key types

| Type | Purpose |
|------|---------|
| `Cli` | Top-level clap parser: instance root, log level, bind, port |
| `Command` | Subcommand enum: Health, Backup, Maintenance, Memory, Tui, etc. |
| `NousDaemonBridge` | Implements `DaemonBridge` to connect oikonomos tasks to nous actors |
| `FilesystemPlanningService` | Implements `PlanningService` trait over dianoia workspace |
| `KnowledgeSearchAdapter` | Bridges organon tool search to mneme knowledge store |

## Public API surface

- `aletheia::main` - binary entry point (not a library; no public API surface)
- `crates/aletheia/src/commands/` - one module per CLI subcommand
- `crates/aletheia/src/dispatch.rs` - inbound message dispatcher (agora → nous routing)

## When to look here

- When adding a new CLI subcommand
- When modifying service startup order or adapter wiring between crates
