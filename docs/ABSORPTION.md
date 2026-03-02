# ABSORPTION.md

Audit trail for the CozoDB 0.7.6 absorption into Aletheia (Phase 1–5).

## Lines Removed

| Metric | Count |
|--------|-------|
| Original cozo-core 0.7.6 source (estimated) | ~45,000 lines |
| Current `mneme-engine/src` | 42,117 lines |
| Current `graph-builder/src` | 3,673 lines |
| Stopwords removed (CJK/multilingual) | ~20,582 lines |
| Effective production source | ~42,117 lines |

Key removals from cozo-core 0.7.6:
- Storage backends: `storage/rocks.rs` (legacy), `storage/sqlite.rs`, `storage/sled.rs`, `storage/tikv.rs`
- Chinese tokenizer: `fts/cangjie/`, jieba-rs dependency (~4,000 lines)
- FFI bindings: `DbInstance` dispatch, all `*_str` foreign-callable methods
- HTTP fetch utility: `fixed_rule/utilities/jlines.rs`, minreq dependency
- CSV reader utility: `fixed_rule/utilities/csv.rs`, csv dependency
- Stopwords trimmed to English-only (from 21,885 lines to ~1,303 lines)
- `lib.rs` rewritten as typed `Db` enum facade replacing `DbInstance`

## Unsafe Sites Carried

| Crate | Sites (no SAFETY comment) | Location Summary |
|-------|---------------------------|------------------|
| mneme-engine | 21 | `query/graph.rs` (6), `data/value.rs` (4), `runtime/minhash_lsh.rs` (2), `query/reorder.rs` (2), `data/relation.rs` (2), `data/memcmp.rs` (2), `data/functions.rs` (2), `storage/newrocks.rs` (1) |
| graph-builder | 27 | CSR construction, rayon parallel iteration |

All Phase 2 unsafe sites were audited and either documented with `// SAFETY:` comments or left pending the backlog below. The 21 remaining mneme-engine sites without SAFETY comments are primarily pointer-level operations from the original CozoDB source (`from_shape_ptr`, ndarray transmutes, bytemuck casts).

## Unwraps Remaining

| Category | Count | Disposition |
|----------|-------|-------------|
| `runtime/tests.rs` (test assertions) | 210 | Acceptable |
| `data/tests/` (test assertions) | 523 | Acceptable |
| `lib.rs` test helper section | 7 | Acceptable (test-only block) |
| `data/memcmp.rs` (hot serialization) | 47 | Deferred — write-to-Vec never fails; see backlog |
| Infallible Mutex/RwLock — `db.rs` | 22 | Annotated `// INVARIANT: lock is not poisoned` |
| Infallible Mutex/RwLock — `storage/mem.rs` | 3 | Annotated `// INVARIANT: lock is not poisoned` |
| Infallible Mutex/RwLock — `runtime/relation.rs` | 2 | Annotated `// INVARIANT: lock is not poisoned` |
| HNSW filter_map DataValue extraction | 14 | Annotated `// INVARIANT: stored by HNSW insert path` |
| `runtime/relation.rs` storage deserialization | 1 | Annotated `// INVARIANT: storage layer writes well-formed msgpack` |
| Remaining non-test production (data/, query/) | ~25 | See below — infallible by construction or deferred |

### Remaining Production Unwraps by File

| File | Count | Notes |
|------|-------|-------|
| `data/memcmp.rs` | 47 | Write-to-`Vec<u8>` never fails — deferred to backlog |
| `data/functions.rs` | ~15 | Mix: ndarray shape (invariant), UNIX_EPOCH (infallible), validated input paths |
| `data/expr.rs` | ~4 | Stack pop after push — infallible by construction |
| `data/value.rs` | 2 | `as_slice()` on contiguous ndarray — infallible, noted with SAFETY comment |
| `data/aggr.rs` | 2 | Tuple index access after length check |
| `data/json.rs` | 2 | `as_slice()` on contiguous ndarray — infallible |
| `query/ra.rs` | ~15 | Filter store-map lookups, infallible binding maps |

## Cleanup Backlog

### Cargo.toml Lint Suppressions

| Suppression | Reason | Priority |
|-------------|--------|----------|
| `mutable_key_type` | `DataValue` used as hash key (intentional CozoDB pattern) | Low |
| `type_complexity` | Deeply nested generic types in query engine | Low |
| `too_many_arguments` | CozoDB function signatures (>7 params) | Medium |
| `dead_code` | Unreachable code from stripped storage backends | Medium |
| `private_interfaces` | `pub(crate)` types in `pub` trait impls | Low |
| `unsafe_code` | ndarray + bytemuck in data layer, covered by Phase 2 audit | Low |
| `unexpected_cfgs` | Orphaned `cfg` guards for stripped backends | Low |
| `rust_2018_idioms` | CozoDB used 2018-era patterns throughout | Low |
| `pedantic` (clippy) | Bulk-suppressed; individual items need triage | Medium |
| `get_first`, `iter_kv_map` | CozoDB iteration patterns | Low |

### Deferred Unwrap Conversions

- **`data/memcmp.rs` (47 sites):** Write-to-`Vec<u8>` is infallible (`Write for Vec<u8>` never errors). Conversion would add noise with no safety benefit. Document and leave. Future: add a newtype wrapper that makes infallibility explicit.
- **`from_shape_ptr` alignment hardening (AD-18):** `data/value.rs` uses `from_shape_ptr` for ndarray construction from raw pointers. Alignment is guaranteed by the allocation path but not statically asserted. Add `assert_eq!(ptr.align_of(), align_of::<T>())` guard.
- **`query/ra.rs` store-map lookups (~15):** These are `HashMap::get(key).unwrap()` where the key was inserted in the same query compilation pass. Infallible by construction but not proven via types. Future: typed key proof via index newtype.
- **Per-module snafu Error enum hierarchy:** Current `DbResult<T> = Result<T, BoxErr>` erases error types at module boundaries. Future refactor: per-module snafu enums for `query/`, `runtime/`, `storage/` propagating with typed `.context()` calls.
