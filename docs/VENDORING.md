# Vendored Dependencies

## What's Vendored

### mneme-engine (from CozoDB)

| Field | Value |
|-------|-------|
| Original project | CozoDB |
| Original crate | cozo-core |
| Version | 0.7.6 |
| Source | https://github.com/cozodb/cozo |
| License | MPL-2.0 |
| Copyright | Copyright 2022-2024 Ziyang Hu and CozoDB contributors |
| Absorbed to | `crates/mneme-engine/` |

### graph-builder (from neo4j-labs/graph)

| Field | Value |
|-------|-------|
| Original project | graph (neo4j-labs) |
| Original crate | graph_builder |
| Version | 0.4.1 |
| Source | https://github.com/neo4j-labs/graph |
| License | MIT |
| Copyright | Copyright (c) neo4j-labs contributors |
| Absorbed to | `crates/graph-builder/` |

## License Compliance

Per MPL-2.0 Section 3.1, source files from CozoDB retain their original license. MPL-2.0 is compatible with Aletheia's AGPL-3.0-or-later per Section 3.3 (Secondary License). Copyright headers in absorbed source files are preserved verbatim.

graph-builder is MIT-licensed. No additional compliance requirements beyond preserving the copyright notice.

## Modifications from Original

### mneme-engine

- Storage backends removed: `rocks.rs` (legacy), `sqlite.rs`, `sled.rs`, `tikv.rs`
- Chinese tokenizer removed: `fts/cangjie/`, jieba-rs dependency (~4,000 lines)
- FFI/binding code removed: `DbInstance`, all `*_str` methods
- HTTP fetch utility removed: `fixed_rule/utilities/jlines.rs`, minreq dependency
- CSV reader utility removed: `fixed_rule/utilities/csv.rs`, csv dependency
- Stopwords trimmed to English-only (from 21,885 lines to ~1,303 lines)
- `lib.rs` rewritten: new `Db` facade enum replacing `DbInstance`
- `env_logger` moved to dev-dependencies

### graph-builder

- `compat.rs` removed (polyfills replaced with stdlib equivalents)
- `build.rs` removed (feature probes for pre-1.80 Rust no longer needed)
- Unused input formats removed: dotgraph, gdl, graph500, binary
- `adj_list.rs` removed (only CSR graphs used)
- rayon pinned to =1.10.0 (1.11 breaks `EdgeList::edges()`)

## Upstream Status

### CozoDB

| Field | Value |
|-------|-------|
| Repository | https://github.com/cozodb/cozo |
| Version absorbed | 0.7.6 (cozo-core) |
| Last upstream commit | 2024-12-04 |
| Upstream status | Inactive (no commits since Dec 2024) |

**Relevant issues:**
- **#298** - rayon 1.11 breaks graph_builder compilation. Pinned rayon to =1.10.0.
- **#287** - env_logger in non-dev dependencies. Moved to dev-dependencies in absorption.

No unmerged PRs contain fixes we need. Upstream is inactive - no divergence risk. Future CozoDB development (if any) would need manual review for cherry-pick into mneme-engine.

### graph_builder

| Field | Value |
|-------|-------|
| Repository | https://github.com/neo4j-labs/graph |
| Version absorbed | 0.4.1 |
| Upstream status | Inactive |

**Relevant issues:**
- **graph#138** - rayon 1.11 type mismatch in `EdgeList::edges()`. Pinned rayon to =1.10.0.

Upstream inactive. graph_builder 0.4.1 is the final version used by CozoDB. The `graph` facade crate (0.3.1) remains a crates.io dependency for PageRank.

## Cleanup Backlog

### Unsafe Sites

| Crate | Sites (no SAFETY comment) | Location Summary |
|-------|---------------------------|------------------|
| mneme-engine | 21 | `query/graph.rs` (6), `data/value.rs` (4), `runtime/minhash_lsh.rs` (2), `query/reorder.rs` (2), `data/relation.rs` (2), `data/memcmp.rs` (2), `data/functions.rs` (2), `storage/newrocks.rs` (1) |
| graph-builder | 27 | CSR construction, rayon parallel iteration |

The 21 remaining sites are primarily pointer-level operations from the original CozoDB source (`from_shape_ptr`, ndarray transmutes, bytemuck casts).

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

- **`data/memcmp.rs` (47 sites):** Write-to-`Vec<u8>` is infallible. Conversion would add noise with no safety benefit. Future: add a newtype wrapper that makes infallibility explicit.
- **`from_shape_ptr` alignment hardening (AD-18):** `data/value.rs` uses `from_shape_ptr` for ndarray construction from raw pointers. Add `assert_eq!(ptr.align_of(), align_of::<T>())` guard.
- **`query/ra.rs` store-map lookups (~15):** `HashMap::get(key).unwrap()` where the key was inserted in the same compilation pass. Infallible by construction but not proven via types. Future: typed key proof via index newtype.
- **Per-module snafu Error enum hierarchy:** Current `DbResult<T> = Result<T, BoxErr>` erases error types at module boundaries. Future: per-module snafu enums for `query/`, `runtime/`, `storage/`.
