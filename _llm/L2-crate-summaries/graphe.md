# graphe

**Purpose:** Session and message persistence using a fjall LSM-tree store; includes agent portability types for cross-runtime export/import.

## Key types

| Type | Purpose |
|------|---------|
| `SessionStore` | fjall-backed session store (single-writer transactions) |
| `Session` | Session record: id, nous_id, status, type, metrics, timestamps |
| `Message` | Conversation message: role, content, tool calls, token estimate |
| `SessionStatus` | Active, Archived, Distilled |
| `AgentFile` | Portable agent export format (pure data types) |

## Public API surface

- `graphe::store` - `SessionStore` CRUD: create, get, append messages, list, archive
- `graphe::types` - `Session`, `Message`, `UsageRecord`, `Role`, `SessionType`, `SessionStatus`
- `graphe::portability` - `AgentFile` for cross-runtime agent export/import

## When to look here

- When reading or writing session/message data
- When implementing agent portability or backup/restore logic
