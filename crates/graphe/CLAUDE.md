# graphe

## At a glance

Session persistence layer with fjall-backed store. Depends on eidos and koina. Entry point: `src/lib.rs` (SessionStore, Session, Message).

## Depth

Session persistence layer: fjall-backed session/message store with agent portability types. ~2K lines after rusqlite removal (#3446).

## Read first

1. `src/store/mod.rs`: SessionStore dispatch (single fjall backend)
2. `src/store/fjall_store.rs`: fjall LSM-tree implementation
3. `src/types.rs`: Session, Message, UsageRecord, SessionStatus, SessionType, Role
4. `src/portability.rs`: AgentFile format for cross-runtime export/import (data types only)
5. `src/error.rs`: Error enum — Storage/StoredJson/Io plus engine variants

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `SessionStore` | `store/fjall_store.rs` | fjall-backed session store (single-writer tx database) |
| `Session` | `types.rs` | Session record (id, nous_id, status, type, metrics, timestamps) |
| `Message` | `types.rs` | Conversation message (role, content, tool calls, token estimate) |
| `UsageRecord` | `types.rs` | Per-turn token usage and model info |
| `SessionStatus` | `types.rs` | Enum: Active, Archived, Distilled |
| `SessionType` | `types.rs` | Enum: Primary, Background, Ephemeral |
| `Role` | `types.rs` | Enum: User, Assistant, ToolResult, System |
| `AgentFile` | `portability.rs` | Portable agent export format (pure data types; no writer/reader glue) |

## Storage model

fjall is a pure-Rust LSM-tree — no C dependency, no WAL management to tune. Keys are UTF-8 strings and values are JSON-encoded domain structs. See the docstring in `store/fjall_store.rs` for the exact partition/key layout.

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `mneme-engine` | no | Gates Datalog-engine error variants that reference `tokio::task::JoinError` |

## Common tasks

| Task | Where |
|------|-------|
| Add session/message field | `src/types.rs` (struct) + `src/store/fjall_store.rs` (encode/decode) |
| Add portability field | `src/portability.rs` (AgentFile or sub-structs) |
| Add error variant | `src/error.rs` (Error enum) |

## Dependencies

Uses: eidos, koina (with `fjall` feature), fjall, jiff, snafu, tracing
Used by: mneme (facade re-export), episteme
