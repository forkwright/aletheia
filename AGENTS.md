# AGENTS.md

Universal rules for all AI coding tools (Cursor, Copilot, kimi, Claude Code).
For Claude Code-specific commands and workflow, see [CLAUDE.md](CLAUDE.md).

## Error handling

- Use `snafu` with `.context()` propagation and `Location` tracking
- No `unwrap()` in library code  -  use proper error types
- Single error enum per crate boundary

## Lint suppressions

- `#[expect(lint, reason = "...")]`  -  never `#[allow]`
- Every suppression must have a justification in the reason string

## Test data

- All test data MUST use synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- NEVER use real personal information in test fixtures or example data
- CI PII scanner rejects commits with personal data patterns

## Key patterns

- **IDs:** Newtypes for all domain IDs (`AgentId`, `SessionId`, `NousId`)
- **Async:** Tokio actor model (`NousActor` pattern)
- **Naming:** Greek names per [docs/gnomon.md](docs/gnomon.md), registry at [docs/lexicon.md](docs/lexicon.md)
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Config:** figment TOML cascade in `taxis`

## Architecture

- `pub(crate)` by default; `pub` only for cross-crate API surface
- No barrel files  -  import from the file that owns the symbol
- Module imports flow downward: higher layers depend on lower, never reverse
- Operator-specific config belongs in `instance/` (gitignored), not `shared/` or repo root

## Before submitting

1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace`: zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context
5. All lint suppressions use `#[expect]` with reason
