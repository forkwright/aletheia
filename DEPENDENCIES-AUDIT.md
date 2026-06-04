# Dependency Ownership Audit

Last checked: 2026-05-29 on `chore/05d-ring-to-p256`. `chrono` + `aws-lc-sys`
provenance re-verified 2026-06-03 for the v1.0.0 cut-line (criterion 11).

This document records the live dependency graph. It is not a Phase 05d target
statement.

## v1.0.0 Disposition (cut-line criterion 11)

Criterion 11 requires dependency purity to be either closed or
**documented-as-post-1.0**. The following native/unmaintained residue is
**accepted for v1.0.0** — each is transitive, none sits in the "no C++ in the
brain" knowledge/ML core (that path is candle + fjall, pure Rust), and none has
a clean pure-Rust replacement that ships in the 1.0 window:

| Dependency | Why it stays | Removal trigger |
|-----------|--------------|-----------------|
| `aws-lc-sys` (C) | Crypto backend for `rustls`'s `aws-lc-rs` provider (rustls 0.23 default), used by the workspace's TLS. The only pure-Rust rustls provider (`rustls-rustcrypto`) is experimental; switching to `ring` trades one C lib for another. | rustls ships a production pure-Rust provider, or operator accepts `rustls-rustcrypto`. |
| `chrono` (pure Rust) | Non-optional dep of `rmcp`, `lopdf` (via `pdf-extract`), and `spreadsheet-ods`. Pure Rust, so no C-FFI concern; the only issue is duplication with `jiff` (the workspace time crate). | upstreams drop chrono, or the doc/MCP crates are replaced. |
| `rustls-pemfile` (unmaintained, RUSTSEC-2025-0134) | Unmaintained-only (not a vulnerability). Transitive via `qdrant-client → tonic`. Deny-ignored with rationale (`deny.toml`). | `qdrant-client`/`tonic` migrate to `rustls-pki-types` `PemObject` (#4389). |

These are tracked below; the disposition is "accept + track upstream," not
"unresolved blocker."

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

## aws-lc-sys Status (accepted post-1.0 — see Disposition)

`aws-lc-sys` (C) enters the **normal** build graph as the crypto backend of
`rustls`'s `aws-lc-rs` provider — the default for rustls 0.23. The workspace's
TLS-using crates (`aletheia`, `episteme`, and their consumers) depend on
`rustls v0.23` directly, so the path is
`aws-lc-sys ← aws-lc-rs ← rustls v0.23 ← {aletheia, episteme, ...}`
(verified 2026-06-03 via `cargo tree --workspace --locked -i aws-lc-sys`).

This corrects an earlier note that attributed `aws-lc-sys` only to a `ureq`
dev-dependency feature bleed — it is in fact in the primary TLS path, not a
dev-only artifact. Eliminating it requires a different rustls crypto provider:
`ring` (still C) or `rustls-rustcrypto` (pure Rust, currently experimental).
Neither is a clean v1.0.0 move, so it is accepted as post-1.0 residue (see the
v1.0.0 Disposition table above).

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

## Remaining (accepted post-1.0 — see v1.0.0 Disposition)

These are no longer "blockers" for the release: the v1.0.0 Disposition (top of
this doc) accepts them as tracked post-1.0 residue.

| Dependency | Removal blocker | Tracking |
|-----------|-----------------|---------|
| `aws-lc-sys` | needs a production pure-Rust rustls crypto provider | W-04 / REQ-05d-01..03 |
| `chrono` | rmcp, lopdf, spreadsheet-ods have non-optional chrono deps | REQ-05d-06 |

## Commands Used

```bash
cargo tree -p aletheia --locked -i ring
cargo tree -p aletheia --locked -i openssl-sys
cargo tree --workspace --locked -i chrono
cargo tree --workspace --locked -i rusqlite
cargo tree --workspace --locked -i aws-lc-sys
```
