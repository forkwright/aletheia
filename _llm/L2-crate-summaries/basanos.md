# basanos

**Purpose:** Planning and standards linter for kanon projects.

## Key types

| Type | Purpose |
|------|---------|
| `AuditReport` | Current public type or boundary; see L3/source for exact fields |
| `AuditCheck` | Current public type or boundary; see L3/source for exact fields |
| `CheckResult` | Current public type or boundary; see L3/source for exact fields |
| `run_audit_component` | Current public type or boundary; see L3/source for exact fields |
| `run_lint` | Current public type or boundary; see L3/source for exact fields |

## Public API surface

- `basanos::commands/audit_component` - public items from `src/commands/audit_component.rs`
- `basanos::commands` - public items from `src/commands/mod.rs`
- `basanos::error` - public items from `src/error.rs`
- `basanos::rules/api_consistency` - public items from `src/rules/api_consistency.rs`
- `basanos::rules/architecture/fact_required` - public items from `src/rules/architecture/fact_required.rs`

## When to look here

- When work touches `crates/basanos` or downstream imports from `basanos`.
- For exact signatures, load `_llm/L3-api-index/basanos.md` if present, then source.

## Recent changes

L3 was refreshed for the standards/planning linter after today's workspace-wide API movement.
