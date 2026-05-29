# Dependency Ownership Audit

Last checked: 2026-05-29 on `chore/05d-ring-to-p256`.

This document records the live dependency graph. It is not a Phase 05d target
statement.

## Current C / Native Footprint

- `ring` is now only transitive via `rustls` (removed from `crates/aletheia`
  direct deps in this branch). `cargo tree -p aletheia --locked -i ring` shows
  ring reachable only through `rustls v0.23`.
- `openssl-sys` is not present in the locked `aletheia` graph at this revision:
  `cargo tree -p aletheia --locked -i openssl-sys` reports no matching package.
- `ort` remains in the workspace as an optional classifier dependency via
  `crates/episteme` feature wiring, but the locked default `aletheia` graph does
  not include the former `openssl-sys` path.
- `chrono` remains reachable in the workspace. The `cron` crate vector was
  closed by #3898, but chrono itself is still pulled in by:
  - `rmcp v1.5.0` (non-optional dep, `mcp` feature in `crates/aletheia`)
  - `lopdf v0.38.0` via `pdf-extract` → `crates/poiesis/inspect`
  - `spreadsheet-ods v1.0.4` → `crates/poiesis/sheet`
  These are external crates with non-optional chrono dependencies that cannot
  be disabled from our workspace. Eliminating chrono requires upstream fixes or
  crate replacements — a design decision outside the current scope.
- `rusqlite` remains reachable through:
  - `crates/gnosis`, the live code-graph index.
  - `crates/aletheia-sessions-migrate`, the legacy SQLite-to-fjall session
    migration tool.

## aws-lc-sys Status (Blocked)

`aws-lc-sys` appears in `Cargo.lock` as a transitive dep of `aws-lc-rs`, which
is pulled in by `ureq v3.3.0` via its `[dev-dependencies]`:

```
[dev-dependencies]
rustls = { version = "0.23", features = ["aws-lc-rs"] }
```

With Cargo resolver v2, dev-dependency *features* of external crates still
bleed into `Cargo.lock` even though they are not used in normal builds.
`hf-hub` (used by mneme/embed-candle) depends on `ureq 3.3.0`. Eliminating
`aws-lc-sys` requires either: (a) switching `hf-hub` from `ureq` to its
`tokio` feature; or (b) patching `ureq` locally. Both are design decisions.

## Storage Status

The live session and auth stores are fjall-backed. Statements that `rusqlite` was
removed from the whole stack are too broad; the accurate statement is that
`rusqlite` was removed from the live session/auth storage path, while gnosis and
the legacy migrator still use it intentionally.

## Fjall Ownership Recommendation

**Recommendation: retain fjall as the long-term storage backend.**

fjall v3.1.4 dependency profile:

- All transitive deps are pure Rust (byteorder-lite, byteview, dashmap, flume,
  lsm-tree, lz4_flex, xxhash-rust) — no C FFI, no build-script native compilation.
- This satisfies the phase 05d purity goal: live data paths have no C dependency chain.

**Remaining rusqlite consumers** (not targeted by phase 05d):

| Crate | Use | Migration path |
|-------|-----|---------------|
| `crates/gnosis` | Code-graph index | Design decision: evaluate fjall or sqlite3 via `rusqlite` as long-term index store |
| `crates/aletheia-sessions-migrate` | One-shot SQLite→fjall migrator | Retire when all instances are migrated |

The gnosis rusqlite usage is intentional and not an immediate purity concern — gnosis is a
dev/analysis tool and C FFI in a developer-only crate is a lower risk than in the live server
path. A follow-on decision is needed before migrating gnosis storage.

## Completed Eliminations

| Dependency | Eliminated in | Method |
|-----------|---------------|--------|
| `cron` (chrono-dep) | v0.26.x / #3898 | Replaced with purpose-built jiff-based parser |
| `ring` (direct dep of `crates/aletheia`) | this branch | Migrated `tls_self_signed.rs` to `p256` (RustCrypto) |
| `ring` direct dep in `crates/symbolon` | prior to 2026-05-08 | Migrated to p256/hmac/sha2/rand |

## Remaining (Blocked on Design Decisions)

| Dependency | Blocker | Tracking |
|-----------|---------|---------|
| `aws-lc-sys` | `ureq 3.3.0` dev-dep feature bleed; hf-hub feature switch needed | W-04 / REQ-05d-01..03 |
| `chrono` | rmcp, lopdf, spreadsheet-ods have non-optional chrono deps | REQ-05d-06 |

## Commands Used

```bash
cargo tree -p aletheia --locked -i ring
cargo tree -p aletheia --locked -i openssl-sys
cargo tree --workspace --locked -i chrono
cargo tree --workspace --locked -i rusqlite
cargo tree --workspace --locked -i aws-lc-sys
```
