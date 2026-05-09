# Dependency Ownership Audit

Last checked: 2026-05-08 on `fix/config-doc-drift-cluster`.

This document records the live dependency graph. It is not a Phase 05d target
statement.

## Current C / Native Footprint

- `ring` is still a direct dependency of `crates/aletheia` for TLS self-signed
  certificate generation and also appears transitively through `rustls`.
- `openssl-sys` is not present in the locked `aletheia` graph at this revision:
  `cargo tree -p aletheia --locked -i openssl-sys` reports no matching package.
- `ort` remains in the workspace as an optional classifier dependency via
  `crates/episteme` feature wiring, but the locked default `aletheia` graph does
  not include the former `openssl-sys` path.
- `chrono` remains reachable in the workspace through `energeia` / `cron` and
  through third-party document/metadata crates. It is not yet eliminated from
  `Cargo.lock`.
- `rusqlite` remains reachable through:
  - `crates/gnosis`, the live code-graph index.
  - `crates/aletheia-sessions-migrate`, the legacy SQLite-to-fjall session
    migration tool.

## Storage Status

The live session and auth stores are fjall-backed. Statements that `rusqlite` was
removed from the whole stack are too broad; the accurate statement is that
`rusqlite` was removed from the live session/auth storage path, while gnosis and
the legacy migrator still use it intentionally.

## Commands Used

```bash
cargo tree -p aletheia --locked -i ring
cargo tree -p aletheia --locked -i openssl-sys
cargo tree --workspace --locked -i chrono
cargo tree --workspace --locked -i rusqlite
```
