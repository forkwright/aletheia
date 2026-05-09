# aletheia-sessions-migrate

**Purpose:** One-shot SQLite v32 -> fjall sessions-store migrator for legacy aletheia 0.15.x instances.

## Key types

| Type | Purpose |
|------|---------|
| `print_dry_run` | Current public type or boundary; see L3/source for exact fields |
| `print_migration` | Current public type or boundary; see L3/source for exact fields |
| `print_verification` | Current public type or boundary; see L3/source for exact fields |
| `ALL_PARTITIONS` | Current public type or boundary; see L3/source for exact fields |
| `Destination` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `aletheia-sessions-migrate::commands/report` - public items from `src/commands/report.rs`
- `aletheia-sessions-migrate::dest` - public items from `src/dest.rs`
- `aletheia-sessions-migrate::error` - public items from `src/error.rs`
- `aletheia-sessions-migrate::migrate` - public items from `src/migrate.rs`
- `aletheia-sessions-migrate::schema` - public items from `src/schema.rs`

## When to look here

- When work touches `crates/aletheia-sessions-migrate` or downstream imports from `aletheia-sessions-migrate`.
- For exact signatures, load `_llm/L3-api-index/aletheia-sessions-migrate.md` if present, then source.

## Recent changes

The legacy sessions migrator is indexed so pre-fjall upgrade paths stay visible to agents.
