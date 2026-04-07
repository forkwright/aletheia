# Fjall storage engine evaluation

> As of v3.1.3 (April 2026). Closes #2290. Part of the Ownership / Clean-Room track.

## Verdict

**Keep fjall.** The evidence below supports retaining it as the storage backend for the unified engine (`krites`). No purpose-built LSM is warranted at this stage. The dependency is well-maintained, pure Rust, dual-licensed, and has a stable disk format guarantee. The two areas requiring ongoing attention are: the single-maintainer bus factor, and the two `unsafe impl Sync` blocks we own in the integration layer.

---

## API surface

fjall exposes a `BTreeMap`-like interface over persistent keyspaces.

| Primitive | Surface |
|-----------|---------|
| `Database::builder(path).open()` | Opens or creates a database |
| `TxDatabase::builder` / `SingleWriterTxDatabase::builder` | Transactional variants |
| `db.keyspace(name, opts)` | Opens or creates a keyspace (column family) |
| `keyspace.insert(k, v)` / `.get(k)` / `.remove(k)` | Point operations |
| `keyspace.prefix(p)` / `.range(r)` | Iterator; implements `DoubleEndedIterator` |
| `db.persist(PersistMode)` | Explicit durability flush |
| `WriteBatch` | Non-transactional batched writes |
| `OptimisticTxDatabase` / `SingleWriterTxDatabase` | MVCC (OCC) and serialized-write transactional modes |
| `compaction::{Leveled, Fifo}` | Pluggable compaction strategies |
| `compaction::filter::Factory` | Custom compaction filters |

All six domain stores used by the unified engine map naturally onto keyspaces within a single `Database`. Cross-keyspace atomic semantics (via `WriteBatch`) cover the multi-store write path.

The API we use is through the `StoreTx` trait in `krites/src/storage/fjall_backend.rs`. It calls `SingleWriterTxDatabase`, `write_tx()`, `read_tx()`, and `Snapshot` exclusively. This is a small, stable slice of the total surface.

---

## API stability

fjall uses semver with a major-version stability guarantee on disk format.

| Aspect | Finding |
|--------|---------|
| Disk format | Stable within a major version. Breaking format changes trigger a major bump and a migration path (stated in README). |
| Semver compliance | Observed: 3.1.1 to 3.1.3 added no breaking changes. Source layout, public types, and feature flags are identical across the three versions in the registry. |
| MSRV | 1.90.0, pinned in `Cargo.toml`. Stable channel only. |
| lz4_flex bump | 3.1.1 used `lz4_flex 0.11.5`; 3.1.3 uses `lz4_flex 0.13.0`. Minor version bump in a transitive dep, no API change to fjall itself. |
| `lsm-tree` pinning | fjall pins `lsm-tree` at `~3.1.x` (tilde range, patch updates only). The two crates are co-versioned and co-released by the same author. |
| Feature flags | `lz4` (default), `bytes_1`, `metrics`, `__internal_whitebox`. All stable within the 3.x line. |

No breaking changes were observed across the three locally cached versions (3.1.1, 3.1.2, 3.1.3). The current `Cargo.toml` pins fjall at `version = "3"`, which allows all 3.x patch and minor updates. Given that the disk format is major-version stable and the API is consistent across the 3.1.x range, this constraint is appropriate. No tighter pinning is required.

---

## Unsafe usage

### In fjall itself

fjall declares `#![deny(unsafe_code)]` in `src/lib.rs`. The codebase contains five `unsafe` references:

- `src/lib.rs:74` -- the `#![deny(unsafe_code)]` lint gate itself
- `src/meta_keyspace.rs:177` -- `#[expect(unsafe_code, clippy::indexing_slicing)]` with one `unsafe` block calling `Slice::builder_unzeroed`. This is uninitialized buffer allocation with a known size; the pattern is a standard optimization for buffer-building in I/O code.
- `src/journal/entry.rs:199` -- `#[warn(unsafe_code)]` with one `unsafe { Slice::builder_unzeroed(value_len) }` call. Same pattern as above.

All three `unsafe` call sites are `Slice::builder_unzeroed`, an uninitialized-allocation helper from `byteview`. The scope is narrow, the rationale is I/O performance, and all three are annotated with lint attributes.

### In lsm-tree (direct dep of fjall)

`lsm-tree` contains 52 `unsafe` references. These fall into two categories:

- **Slice indexing via `get_unchecked`** in `data_block/mod.rs` and `table/util.rs`. All are guarded by bounds-checked logic in the same function and annotated with `#[expect(unsafe_code, reason = "...")]`. These are standard hot-path optimizations for block decoding.
- **Hash index builder** in `block/hash_index/builder.rs` and `block/hash_index/reader.rs`. Uses `get_unchecked` for bucket access. Bounds correctness is established by the hash index construction invariants.

The unsafe usage in `lsm-tree` is consistent with what a well-maintained low-level storage library requires for performance. It is not systemic or defensive; each site is localized and annotated.

### In our integration layer

`krites/src/storage/fjall_backend.rs` contains two `unsafe impl Sync` declarations:

```rust
// SAFETY: fjall's Snapshot is a read-only view with no interior mutability, so
// sharing a reference across threads is sound. SingleWriterWriteTx is protected
// by an external mutex guard at the call site, ensuring exclusive access while
// the reference is live. Both impls are therefore safe to declare Sync.
unsafe impl Sync for FjallReadTx<'_> {}
unsafe impl Sync for FjallWriteTx<'_> {}
```

These are required because fjall's transaction types do not implement `Sync` -- the `StoreTx` trait requires `Sync`. The soundness argument in the comment is correct: `FjallReadTx` wraps a `Snapshot` (immutable view, no interior mutability) and `FjallWriteTx` is protected by an external serialization constraint (`SingleWriterTxDatabase` serializes writes). The safety invariants are maintained by the `Storage` trait contract, not by the types themselves.

This is a known structural mismatch between the `StoreTx` trait's `Sync` bound and fjall's transaction design. It does not represent unsafe code in fjall; it represents an adaptation cost in our layer. If fjall's types gain `Sync` impls in a future release, these declarations can be removed.

---

## Compaction

fjall delegates compaction to `lsm-tree` and runs it in background threads.

| Property | Detail |
|----------|--------|
| Background | Yes. A `WorkerPool` runs compaction on dedicated threads (default: `min(available_cores, 4)`). Configurable via `Config::worker_threads`. |
| Trigger | Automatic. The `Supervisor` schedules compaction tasks. Keyspaces are checked on each compaction cycle. Write stall is applied if compaction falls behind writes. |
| Strategies | `Leveled` (default, equivalent to LevelDB/RocksDB leveled), `Fifo` (first-in-first-out, for time-series data). `SizeTiered` is present in `lsm-tree` source but commented out. |
| Configurability | Per keyspace via `KeyspaceCreateOptions`. Block sizes, compression, filter policy, index partitioning, KV separation, and compaction filter factory are all configurable. |
| Compaction filters | A `Filter` trait allows custom logic during compaction (e.g., TTL expiry, tombstone cleanup). The `filter` submodule is public and stable. |
| Journal flushing | Journal is flushed to disk when it exceeds `max_journaling_size_in_bytes` (default 512 MiB). Can be configured; `manual_journal_persist` is available. |
| Durability | `PersistMode::SyncAll`, `PersistMode::SyncData`, or buffered (OS-level). Default is OS-buffered. The database performs `SyncAll` on `Drop`. |

The compaction design is sound for our access pattern. The knowledge engine performs random point reads and range scans over a bounded dataset (per-nous fact stores). Neither a FIFO nor a size-tiered strategy is warranted. Leveled compaction minimizes read amplification, which matches the read-heavy recall pipeline.

The `range_compact` method in the `Storage` trait is a no-op in our fjall backend (`Ok(())`). fjall does not expose manual range compaction. This is acceptable because the background compaction is automatic and the workload does not require manual compaction windows.

---

## Maintenance

| Metric | Detail |
|--------|--------|
| Author | Marvin Tanquette (`marvin-j97`), Netherlands. Solo primary maintainer. |
| Organization | `fjall-rs` GitHub organization. |
| License | MIT OR Apache-2.0. Compatible with our MIT/Apache-2.0 dual license. |
| Age | Active since 2024. Fjall 1.0 released 2024; 3.x released within the same year based on the version progression. |
| Release cadence | Three patch releases observed in the registry (3.1.1, 3.1.2, 3.1.3). The `lsm-tree` companion crate follows the same cadence. |
| Sponsorship | `orbitinghail` (SQLSync project) listed as sponsor in the README. Indicates external adoption. |
| Community | Discord server, Bluesky presence. `help wanted` label is used on GitHub issues. |
| MSRV policy | Tracks stable Rust with a declared MSRV (1.90.0). Not nightly-only. |
| Dependency footprint | Six direct runtime deps: `byteorder-lite`, `byteview`, `lsm-tree`, `log`, `dashmap`, `xxhash-rust`. Lean. |

**Bus factor is 1.** The codebase has a single primary maintainer. This is a meaningful risk for a storage dependency. The mitigations:

1. MIT OR Apache-2.0 licensing means a fork is always available without legal friction.
2. The disk format is documented and major-version stable, so a fork would not need to migrate existing data.
3. `lsm-tree` is a separable crate; the storage layer could be replaced without touching the rest of the stack.
4. Our integration is behind a feature flag (`storage-fjall`) with a trait boundary (`Storage`, `StoreTx`). Swapping the backend requires changes only in `krites/src/storage/`.

The bus factor is the most significant risk, but it is bounded by the license and trait architecture. Monitor the upstream repository for signs of reduced activity.

---

## Comparison

| Property | fjall 3.x | sled 0.34 | rocksdb (via rust-rocksdb) | SQLite (rusqlite) |
|----------|-----------|-----------|---------------------------|-------------------|
| Language | Pure Rust | Pure Rust | C++ (FFI wrapper) | C (FFI wrapper) |
| C deps | None | None | Yes (librocksdb, libsnappy, liblz4) | Yes (bundled) |
| Binary size impact | Low | Low | High (RocksDB is ~4MB compiled) | Medium (bundled ~2MB) |
| License | MIT/Apache-2.0 | MIT/Apache-2.0 | Apache-2.0 | Public domain |
| Embeddable | Yes | Yes | Yes | Yes |
| LSM-tree | Yes | Yes | Yes (RocksDB) | No (B-tree) |
| Multiple column families | Yes (keyspaces) | No | Yes | No (separate tables, one WAL) |
| Serializable transactions | Yes (OCC or single-writer) | Planned, limited | Yes | Yes (WAL serialized) |
| Compaction strategies | Leveled, FIFO | Size-tiered only | Many (Level, Universal, FIFO, etc.) | N/A |
| Custom compaction filters | Yes | No | Yes | N/A |
| KV separation (large values) | Yes | No | Yes (BlobDB) | N/A |
| Maintenance | Active (2024-present) | Unmaintained (stale since 2022) | Active (Meta) | Active (SQLite team) |
| Bus factor | 1 | Abandoned | Large team | Large team |
| Disk format stability | Major-version guarantee | Unstable (breaking changes in 0.x) | Stable | Stable |
| Unsafe in library | 2 sites (scoped) | Extensive | Extensive (C++) | N/A (C) |
| Current use in project | krites storage backend | None | None | graphe (sessions) |

**sled** is ruled out. Development stalled in 2022. The 0.x version series has no disk format guarantee and a history of breaking changes. It should not be considered.

**rocksdb-rs** brings C++ and a large build artifact. It would break the single-static-binary goal and add cross-compilation complexity. The configurability advantage over fjall does not justify this.

**SQLite** is already used for sessions (`graphe`). It is not an LSM-tree and lacks native column families. Adapting it to replace fjall for the knowledge engine would require either a single large table (losing key isolation) or multiple database files (losing cross-store atomics). Neither is appropriate.

---

## Risk register

| Risk | Severity | Likelihood | Mitigation |
|------|----------|------------|------------|
| Single maintainer goes inactive | High | Low-medium | MIT/Apache-2.0 fork rights; trait boundary in `krites` allows swap |
| `unsafe impl Sync` in our layer unsound | High | Low | Soundness argument is documented; `SingleWriterTxDatabase` serializes writes by design |
| lsm-tree major version break | Medium | Low | Disk format stable within major; migration path is documented upstream |
| Background compaction interferes with latency | Low | Low | Worker threads are bounded (default: 4); compaction runs independently of reads |
| `range_compact` no-op silently skips optimization | Low | Very low | Workload is read-heavy; automatic compaction is sufficient |

---

## Recommendation

Retain fjall as the storage backend for `krites`. No alternatives offer better risk-adjusted properties given the constraints (pure Rust, embeddable, multiple keyspaces, transactions, stable format).

Two follow-up actions:

1. **Watch upstream.** If `fjall-rs/fjall` shows inactivity for three or more months, file an issue to evaluate forking or replacement. Track at the quarterly dependency audit.
2. **Revisit `unsafe impl Sync` if fjall adds `Sync` bounds to transaction types.** The `#[expect(unsafe_code, reason = "...")]` annotations in `fjall_backend.rs` are correct signals; remove them if the upstream types make them unnecessary.

---

## Method

- Source: locally cached versions `fjall-3.1.1`, `fjall-3.1.2`, `fjall-3.1.3` and `lsm-tree-3.1.1` from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.
- Unsafe count: `grep -rn 'unsafe' <src>/` on each crate's `src/` directory.
- API surface: `src/lib.rs`, `src/db.rs`, `src/db_config.rs`, `src/keyspace/mod.rs`, `src/compaction/mod.rs`, `src/compaction/worker.rs`.
- Integration: `crates/krites/src/storage/fjall_backend.rs`, `crates/krites/Cargo.toml`.
- Comparison: crates.io published data, `lsm-tree/src/compaction/mod.rs` for strategy inventory.
- fjall version in Cargo.lock: `3.1.3` (checksum `fdf46551c9abc5fb0e0d540da36c875197285af2a29833892d7d3434b8617343`).
