# graphe

Session persistence layer: SQLite-backed session/message store with backup, migration, recovery, and agent portability. 10.5K lines.

## Read first

1. `src/store/mod.rs`: SessionStore (WAL mode SQLite, recovery, disk monitoring)
2. `src/types.rs`: Session, Message, UsageRecord, SessionStatus, SessionType, Role
3. `src/migration.rs`: Versioned schema migration runner with checksum verification
4. `src/portability.rs`: AgentFile format for cross-runtime export/import
5. `src/recovery.rs`: Corruption detection, read-only fallback, auto-repair
6. `src/backup.rs`: BackupManager with VACUUM INTO and JSON export

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `SessionStore` | `store/mod.rs` | SQLite connection with WAL mode, disk monitoring, recovery modes |
| `Session` | `types.rs` | Session record (id, nous_id, status, type, metrics, timestamps) |
| `Message` | `types.rs` | Conversation message (role, content, tool calls, token estimate) |
| `UsageRecord` | `types.rs` | Per-turn token usage and model info |
| `SessionStatus` | `types.rs` | Enum: Active, Archived, Distilled |
| `SessionType` | `types.rs` | Enum: Primary, Background, Ephemeral |
| `Role` | `types.rs` | Enum: User, Assistant, System |
| `AgentFile` | `portability.rs` | Portable agent export format (sessions, workspace, memory, knowledge) |
| `BackupManager` | `backup.rs` | VACUUM INTO backups, JSON export, disk space checks |
| `Migration` | `migration.rs` | Versioned DDL with up/down SQL and checksum verification |
| `ConnectionHook` | `store/mod.rs` | Trait for connection lifecycle observation (acquire/release) |
| `RecoveryConfig` | `recovery.rs` | Integrity check and auto-repair configuration |
| `RetentionPolicy` | `retention.rs` | Automated cleanup of old sessions and messages |

## Store sub-modules

| Module | Responsibility |
|--------|---------------|
| `store/session.rs` | Session CRUD operations |
| `store/message.rs` | Message history, distillation pipeline, usage recording |
| `store/peripherals.rs` | Agent notes and blackboard |

## Patterns

- **WAL mode**: SQLite write-ahead logging for concurrent read/write access.
- **Recovery**: integrity check on open, backup corrupt DB, attempt repair, fallback to read-only.
- **Disk monitoring**: optional `DiskSpaceMonitor` blocks writes when disk is critically low.
- **Versioned migrations**: monotonic version numbers, SHA-256 checksums, up/down SQL pairs.
- **Path validation**: backup paths sanitized against SQL injection (VACUUM INTO limitation).

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `sqlite` | yes | SQLite session store (rusqlite) |
| `mneme-engine` | no | Error variants referencing tokio types |
| `hnsw_rs` | no | HNSW-related type gating |

## Common tasks

| Task | Where |
|------|-------|
| Add session/message field | `src/types.rs` (struct) + `src/schema.rs` (DDL) + `src/migration.rs` (new migration) + `src/store/` (queries) |
| Add migration | `src/migration.rs` (append to MIGRATIONS array) |
| Modify backup | `src/backup.rs` (BackupManager) |
| Add portability field | `src/portability.rs` (AgentFile or sub-structs) |
| Modify recovery | `src/recovery.rs` (RecoveryConfig, StoreMode) |
| Add retention rule | `src/retention.rs` (RetentionPolicy) |

## Dependencies

Uses: eidos, koina, rusqlite, jiff, snafu, tracing
Used by: mneme (facade re-export), episteme
