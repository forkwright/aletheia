# koina

Core types, errors, tracing, and system abstractions shared by every Aletheia crate. 4K lines. Zero internal dependencies.

## Read first

1. `src/id.rs`: Newtype ID wrappers (NousId, SessionId, TurnId, ToolName) + `newtype_id!` macro
2. `src/error.rs`: Shared error types (file I/O, JSON, identifiers)
3. `src/secret.rs`: SecretString with zeroize-on-drop and redacted Debug/Display
4. `src/system.rs`: FileSystem, Clock, Environment traits + RealSystem/TestSystem impls
5. `src/event.rs`: InternalEvent trait, EventEmitter (metric + log dual-dispatch)
6. `src/defaults.rs`: Shared constants (token budgets, timeouts, iteration limits)

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `NousId` | `id.rs` | Agent identifier (CompactString newtype) |
| `SessionId` | `id.rs` | Session identifier (UUID newtype) |
| `TurnId` | `id.rs` | Turn counter (u64 newtype) |
| `ToolName` | `id.rs` | Tool name identifier (CompactString newtype) |
| `SecretString` | `secret.rs` | API key/token holder with redacted output and zeroize-on-drop |
| `FileSystem` | `system.rs` | Trait: read, write, exists, list for testable filesystem access |
| `Clock` | `system.rs` | Trait: `now()` for testable time |
| `Environment` | `system.rs` | Trait: `var()`, `current_dir()` for testable env access |
| `RealSystem` | `system.rs` | Production implementation of FileSystem + Clock + Environment |
| `TestSystem` | `system.rs` | In-memory implementation for deterministic tests |
| `InternalEvent` | `event.rs` | Trait: typed event producing both a log line and metric labels |
| `EventEmitter` | `event.rs` | Thread-safe dispatcher for InternalEvent to log + metric sinks |
| `CredentialProvider` | `credential.rs` | Trait for dynamic API key resolution |
| `DiskSpaceMonitor` | `disk_space.rs` | Cached disk status checks with threshold alerts |
| `OutputBuffer` | `output_buffer.rs` | Multi-output pipeline stage buffer |
| `RedactingLayer` | `redacting_layer.rs` | Tracing layer that strips sensitive values before output |
| `CleanupRegistry` | `cleanup.rs` | RAII-based cleanup registration for graceful shutdown |

## Patterns

- **`newtype_id!` macro**: generates serde-transparent ID types with Display, FromStr, AsRef, Borrow, Deref, and equality impls.
- **System abstraction**: `RealSystem` for production, `TestSystem` (in-memory filesystem, controllable clock) for tests. Accept `impl FileSystem` in function signatures.
- **Dual-dispatch events**: `InternalEvent` implementors define both a log message and metric labels. Single `emit()` call writes to both sinks.
- **Redaction pipeline**: `redact::redact_sensitive()` strips API keys, bearer tokens, JWTs via regex. `RedactingLayer` applies this to tracing output.
- **Secret safety**: `SecretString` requires explicit `.expose_secret()` for access. Debug, Display, and Serialize all output `[REDACTED]`.

## Common tasks

| Task | Where |
|------|-------|
| Add domain ID type | `src/id.rs` (use `newtype_id!` macro) |
| Add shared error variant | `src/error.rs` (Error enum) |
| Add shared constant | `src/defaults.rs` |
| Add redaction pattern | `src/redact.rs` (new static Regex) |
| Add system trait method | `src/system.rs` (trait + RealSystem + TestSystem impls) |
| Add credential source | `src/credential.rs` (CredentialSource enum) |

## Dependencies

Uses: compact_str, jiff, serde, snafu, tracing, ulid, uuid, zeroize, rustix, regex
Used by: every other Aletheia crate
