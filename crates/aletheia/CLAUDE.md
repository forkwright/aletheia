# aletheia

Binary entrypoint: CLI, server startup, service wiring, and adapter glue. 8.9K lines.

## Read first

1. `src/main.rs`: Clap CLI definition, subcommand dispatch, daemon fork
2. `src/commands/server/mod.rs`: Default server startup (wires all services together)
3. `src/dispatch.rs`: Inbound message dispatcher (agora -> nous routing)
4. `src/commands/mod.rs`: Subcommand module index and `resolve_oikos` helper
5. `src/daemon_bridge.rs`: NousDaemonBridge (connects oikonomos to nous)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `Cli` | `main.rs` | Top-level clap parser with instance root, log level, bind, port |
| `Command` | `main.rs` | Subcommand enum: Health, Backup, Maintenance, Memory, Tui, etc. |
| `Args` | `commands/server/mod.rs` | Server startup arguments forwarded from CLI |
| `NousDaemonBridge` | `daemon_bridge.rs` | Implements `DaemonBridge` to connect oikonomos tasks to nous actors |
| `FilesystemPlanningService` | `planning_adapter.rs` | Implements `PlanningService` trait over dianoia workspace |
| `KnowledgeSearchAdapter` | `knowledge_adapter.rs` | Bridges organon tool search to mneme knowledge store |
| `KnowledgeMaintenanceAdapter` | `knowledge_maintenance.rs` | Bridges daemon maintenance to mneme knowledge operations |

## Patterns

- **Adapter glue**: binary crate implements bridge traits (DaemonBridge, PlanningService, KnowledgeSearchAdapter) that connect library crates without circular dependencies.
- **Daemon fork**: re-executes the binary with `_ALETHEIA_DAEMON=1` env var to avoid unsafe `fork()` inside tokio runtime.
- **Server startup**: sequential init in `commands/server/mod.rs` - config load, tracing, DB, embedding, session store, nous manager, tool registry, daemon runners, HTTP server.
- **Feature gates**: `recall` for knowledge pipeline, `mcp` for diaporeia, `tui` for terminal dashboard, `embed-candle` for local ML.

## Common tasks

| Task | Where |
|------|-------|
| Add CLI subcommand | `src/main.rs` (Command enum) + new file in `src/commands/` |
| Modify server startup | `src/commands/server/mod.rs` |
| Add bridge adapter | New adapter file in `src/`, implement trait from library crate |
| Change daemon fork | `src/main.rs` (`do_daemon()`) |

## Dependencies

Uses: agora, dianoia, oikonomos, koina, taxis, hermeneus, organon, mneme, nous, symbolon, pylon, dokimion, thesauros, diaporeia (optional), theatron-tui (optional)
Used by: (binary crate, not depended on by other crates)
