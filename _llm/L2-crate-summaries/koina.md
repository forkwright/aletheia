# koina

**Purpose:** Core types, errors, tracing, and system abstractions shared by every Aletheia crate. Zero workspace dependencies.

## Key types

| Type | Purpose |
|------|---------|
| `NousId` | Agent identifier (CompactString newtype) |
| `SessionId` | Session identifier (UUID newtype) |
| `SecretString` | API key/token with redacted output and zeroize-on-drop |
| `FileSystem` | Trait for testable filesystem access |
| `Clock` | Trait for testable time (`now()`) |

## Public API surface

- `koina::id` — `NousId`, `SessionId`, `TurnId`, `ToolName` newtypes; `newtype_id!` macro
- `koina::secret` — `SecretString` for safe credential handling
- `koina::system` — `FileSystem`, `Clock`, `Environment` traits + `RealSystem`/`TestSystem` impls

## When to look here

- When you need a shared identifier type or need to define a new domain ID newtype
- When adding injectable system dependencies (filesystem, clock) for testability
