# CozoDB Absorption Analysis

Analysis of `cozo-core` 0.7.6 internals for absorption into Aletheia's crate structure.
Source: `vendor/cozo/cozo-core/src/` (54,888 lines in 86 `.rs` files + 275-line PEG grammar).
Companion: `vendor/graph_builder/src/` (5,927 lines in 15 `.rs` files).

> **Note:** The `data/` module (~11K lines: data types, values, expressions, relations, aggregations)
> is declared in `lib.rs` as `pub mod data` but was not included in the vendor checkout. It is
> referenced by every other module as a leaf dependency. All analysis below accounts for this —
> `data/` imports nothing and everything imports `data/`. Line counts in the module size table
> reflect only what is present in the vendor checkout.

---

## 1. Internal Module Dependency Map

### Module Sizes

| Module | Lines | Files | Description |
|--------|-------|-------|-------------|
| `fts/` | 29,086 | 22 | Full-text search (89% is stopword/charmap data) |
| `data/` | ~11,000 | ~20 | Core types: DataValue, Tuple, Expr, Symbol, Program (not vendored) |
| `runtime/` | 7,569 | 9 | DB instance, transactions, HNSW, MinHash LSH, relations |
| `query/` | 6,830 | 10 | Query compilation, magic sets, relational algebra, evaluation |
| `fixed_rule/` | 4,413 | 23 | Graph algorithms + utilities (CSV, JSON, sort, constant) |
| `parse/` | 3,189 | 7 | CozoScript parser (pest-based) |
| `storage/` | 3,106 | 8 | Storage trait + 6 backends (mem, rocks, newrocks, sqlite, sled, tikv) |
| `lib.rs` | 664 | 1 | Public API surface, DbInstance enum |
| `utils.rs` | 31 | 1 | `swap_option_result`, `TempCollector` |
| `cozoscript.pest` | 275 | 1 | PEG grammar |
| **Total** | **~66,163** | **~102** | |

### Dependency Matrix

Rows import from columns. `data/` and `utils.rs` are leaf nodes.

| → imports | data | fts | fixed_rule | parse | query | runtime | storage | utils |
|-----------|:----:|:---:|:----------:|:-----:|:-----:|:-------:|:-------:|:-----:|
| **data** | — | | | | | | | |
| **fts** | ✓ | — | | ✓ | | ✓ | | |
| **fixed_rule** | ✓ | | — | ✓ | | ✓ | | |
| **parse** | ✓ | ✓ | ✓ | — | | ✓ | | |
| **query** | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ |
| **runtime** | ✓ | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ |
| **storage** | ✓ | | | | | ✓ | — | ✓ |
| **utils** | | | | | | | | — |
| **lib.rs** | ✓ | | ✓ | ✓ | | ✓ | ✓ | |

### Layered Graph

```
lib.rs (crate root — wires everything, public API)
  │
  ├── runtime ─── imports everything (god module)
  │     │
  ├── query ──── imports data, parse, fixed_rule, runtime, storage, utils, fts
  │     │
  ├── parse ──── imports data, fts, fixed_rule, runtime (AccessLevel only)
  │     │
  ├── fixed_rule ── imports data, parse, runtime
  │     │
  ├── fts ──────── imports data, parse, runtime
  │
  ├── storage ──── imports data, runtime (relation decode), utils
  │
  ├── data ──────── leaf (imports nothing)
  └── utils ─────── leaf (imports nothing)
```

### Separability Assessment

| Module | Can Remove Without Touching Kept Modules? |
|--------|------------------------------------------|
| **fts** | No — 15 files outside `fts/` reference it (see §2) |
| **fixed_rule utilities** (csv, jlines) | Yes — registration entries in `mod.rs` only |
| **storage backends** (sqlite, sled, tikv, legacy rocks) | Yes — fully feature-gated |
| **data** | No — universal leaf dependency, cannot be removed |
| **parse** | No — used by query, runtime, fixed_rule |
| **query** | No — used by runtime |
| **runtime** | No — used by everything except data/utils |
| **storage trait** | No — used by runtime, query |

---

## 2. FTS Removal Feasibility

### Size Breakdown

Total: 29,086 lines across 22 files. However, 89% is data tables:

| Component | Lines | Content |
|-----------|-------|---------|
| `tokenizer/stop_word_filter/stopwords.rs` | 21,885 | Stopword dictionaries (multi-language) |
| `tokenizer/ascii_folding_filter.rs` | 4,047 | Unicode→ASCII character mappings |
| Actual FTS logic | ~3,154 | Tokenizers, indexing, AST, Cangjie Chinese |

### Feature-Gating Status

**Not feature-gated.** FTS is always compiled. The FTS-specific Cargo dependencies are unconditional:

```toml
jieba-rs = "0.7.0"      # Chinese tokenizer
aho-corasick = "1.1.3"
rust-stemmers = "1.2.0"
fast2s = "0.3.1"         # Traditional→Simplified Chinese
swapvec = "0.3.0"
```

### External References to FTS (15 files)

| File | What it imports | Why |
|------|----------------|-----|
| `lib.rs:89` | `pub(crate) mod fts` | Module declaration |
| `runtime/db.rs:42,104` | `TokenizerCache` | `Db<S>` struct field |
| `runtime/transact.rs:17,29` | `TokenizerCache` | `SessionTx` struct field |
| `runtime/relation.rs:27,87` | `FtsIndexManifest` | Stored in `RelationHandle` (serialized to disk) |
| `runtime/minhash_lsh.rs:13-14` | `TextAnalyzer`, `TokenizerConfig` | LSH reuses FTS tokenizer infrastructure |
| `runtime/tests.rs:22` | `TokenizerCache`, `TokenizerConfig` | Test setup |
| `parse/mod.rs:35` | `pub(crate) mod fts` | Submodule declaration |
| `parse/sys.rs:22,46` | `TokenizerConfig`, `CreateFtsIndex` | System op parsing |
| `parse/fts.rs` (entire) | `fts::ast::*` | FTS query parser (164 lines) |
| `query/ra.rs:21,44,977-1066` | `FtsSearch`, `FtsSearchRA` | Relational algebra variant (~90 lines) |
| `query/compile.rs:534-567` | `MagicAtom::FtsSearch` | Compilation branch (~33 lines) |
| `query/magic.rs:183,542` | `MagicAtom::FtsSearch` | Magic set transformation |
| `query/reorder.rs:83,131,165,226` | `NormalFormAtom::FtsSearch` | Query reordering |
| `query/stratify.rs:34` | `NormalFormAtom::FtsSearch` | Stratification |
| `query/stored.rs:26,250-397` | `TextAnalyzer`, FTS index ops | Index maintenance during writes |
| `cozoscript.pest:14-22,260-273` | FTS grammar rules | ~16 lines of PEG grammar |
| `data/program.rs` (not vendored) | `FtsSearch`, `FtsScoreKind` | AST types for FTS queries |

### Critical Coupling: MinHash LSH ↔ FTS Tokenizer

`runtime/minhash_lsh.rs` (390 lines) imports `fts::tokenizer::TextAnalyzer` and `fts::TokenizerConfig`
to tokenize text into n-grams before hashing. The `MinHashLshIndexManifest` struct stores
`TokenizerConfig` fields, which are serialized to disk.

**You cannot delete `fts/` wholesale without also removing LSH or extracting the tokenizer.**

### Removal Options

#### Option A: Remove FTS, Keep LSH (extract tokenizer)

1. Extract `fts/tokenizer/` (plus `TokenizerConfig`, `TokenizerCache` from `fts/mod.rs`) into a standalone `tokenizer/` module
2. Delete `fts/ast.rs`, `fts/indexing.rs`, `fts/cangjie/`, `parse/fts.rs` (851 lines of FTS-specific code)
3. Edit 15 files to remove FTS variants, match arms, and index management
4. Remove from `Cargo.toml`: `jieba-rs`, `fast2s`
5. Net removal: ~26K lines (FTS module minus extracted tokenizer)

#### Option B: Remove Both FTS and LSH (recommended — cleaner)

1. Delete `fts/` entirely (29,086 lines)
2. Delete `parse/fts.rs` (164 lines)
3. Remove `runtime/minhash_lsh.rs` (390 lines)
4. Edit the same 15 files plus remove LSH references from `parse/sys.rs`, `query/ra.rs`, `query/stored.rs`, `runtime/relation.rs`
5. Remove from `Cargo.toml`: `jieba-rs`, `fast2s`, `aho-corasick`, `rust-stemmers`, `swapvec`
6. Net removal: ~29,640 lines + 5 Cargo dependencies

**Recommended: Option B.** MinHash LSH is a niche near-duplicate detector. Aletheia's recall system uses
HNSW cosine similarity, not Jaccard/MinHash. If text similarity search is ever needed, it's better to
add it fresh with a modern approach (embedding-based) than to maintain CozoDB's coupled tokenizer stack.

### Storage Format Impact

`RelationHandle` is serialized via serde. Removing `fts_indices` and `lsh_indices` from the struct changes
the deserialized format. Existing databases will fail to load. Since this is an absorption (not maintaining
compatibility), this is acceptable — but note it for migration planning.

---

## 3. Fixed Rules (Graph Algorithms) Inventory

### Algorithms (`fixed_rule/algos/`)

| # | Algorithm | Registered Name(s) | Lines | External Deps | KG Applicability |
|---|-----------|-------------------|-------|---------------|-----------------|
| 1 | All-pairs shortest path | `BetweennessCentrality`, `ClosenessCentrality` | 176 | graph, rayon, ordered_float, priority_queue | Bridge entity identification — nodes connecting disparate knowledge clusters |
| 2 | A* search | `ShortestPathAStar` | 180 | ordered_float, priority_queue | Goal-directed retrieval with embedding-similarity heuristic |
| 3 | Breadth-first search | `BreadthFirstSearch`, `BFS` | 123 | (none) | N-hop entity expansion — "everything related to X within 3 steps" |
| 4 | Degree centrality | `DegreeCentrality` | 76 | (none) | Hub/authority detection — most-connected entities |
| 5 | Depth-first search | `DepthFirstSearch`, `DFS` | 122 | (none) | Causal chain traversal, dependency path exploration |
| 6 | Kruskal MSF | `MinimumSpanningForestKruskal` | 129 | graph, ordered_float, priority_queue | Knowledge distillation — minimal edge set keeping graph connected |
| 7 | Label propagation | `LabelPropagation` | 97 | graph, rand | Fast community detection — automatic topic clustering |
| 8 | Louvain | `CommunityDetectionLouvain` | 318 | graph (GraphBuilder), log | Hierarchical topic organization ("programming" > "Rust" > "async") |
| 9 | PageRank | `PageRank` | 109 | graph (page_rank, PageRankConfig) | Knowledge importance ranking — authoritative concepts surface first |
| 10 | Prim MST | `MinimumSpanningTreePrim` | 118 | graph, ordered_float, priority_queue | Anchored subgraph extraction from a specific concept |
| 11 | Random walk | `RandomWalk` | 138 | rand | Graph embedding training data (node2vec-style), serendipitous retrieval |
| 12 | Shortest path (BFS) | `ShortestPathBFS` | 174 | (none) | Unweighted explanation chains — "how are A and B related?" |
| 13 | Shortest path (Dijkstra) | `ShortestPathDijkstra` | 432 | graph, rayon, ordered_float, priority_queue, smallvec | Core weighted pathfinding — strongest connection paths |
| 14 | Strongly connected components | `ConnectedComponents`, `SCC` | 149 | graph | Circular reference detection, knowledge island identification |
| 15 | Topological sort | `TopSort` | 86 | graph | Dependency ordering, prerequisite sequencing |
| 16 | Triangle counting | `ClusteringCoefficients` | 98 | graph, rayon | Knowledge density measurement — well-established vs sparse areas |
| 17 | Yen's K-shortest | `KShortestPathYen` | 209 | graph, rayon | Multiple explanation paths — diverse reasoning chains |

**Subtotal algorithms:** 3,040 lines across 18 files (17 algos + `mod.rs`).

### Utilities (`fixed_rule/utilities/`)

| # | Utility | Lines | Strip? | Purpose |
|---|---------|-------|--------|---------|
| 1 | `Constant` | 145 | No — core mechanism | Injects literal data into Datalog queries |
| 2 | `CsvReader` | 215 | **Yes** | CSV file/HTTP import |
| 3 | `JsonReader` | 186 | **Yes** | JSON/JSON-Lines file/HTTP import |
| 4 | `ReorderSort` | 153 | No — general utility | Sorting/pagination within Datalog |

**Subtotal utilities:** 716 lines across 5 files. **401 lines strippable** (CSV + JSON readers).

### Dispatch Mechanism

`fixed_rule/mod.rs` (920 lines) registers all algorithms in a `lazy_static!` `DEFAULT_FIXED_RULES` `BTreeMap`.
Each entry maps a string name to `Arc<Box<dyn FixedRule>>`. The `FixedRule` trait requires:
- `arity()` → output tuple width
- `run()` → execute, write results to `RegularTempStore`

Graph algorithms are gated behind `#[cfg(feature = "graph-algo")]`.

### External Dependency Summary

| Crate | Used by | Purpose | Strip candidate? |
|-------|---------|---------|-----------------|
| `graph` (graph_builder) | 12 of 17 algos | CSR graph construction/traversal | No — core of graph algos |
| `rayon` | 5 algos | Parallel computation | No — significant perf benefit |
| `ordered_float` | 5 algos | `OrderedFloat` for priority queues | No — required for `f64` ordering |
| `priority_queue` | 5 algos | Indexed priority queue | No — required for pathfinding |
| `rand` | 2 algos | Random walks, label propagation | No — algorithm requirement |
| `smallvec` | 1 algo (Dijkstra) | Stack-allocated back-pointers | Optional — could use Vec |
| `itertools` | 11 algos + 2 utilities | `.collect_vec()`, `.group_by()` | No — pervasive |
| `csv` | 1 utility (CsvReader) | CSV parsing | **Yes** |
| `minreq` | 2 utilities (gated) | HTTP fetch for import | **Yes** |

---

## 4. Unsafe and Unwrap Audit

### Unsafe Blocks — cozo-core (7 sites)

| File | Line | What | Sound? |
|------|------|------|--------|
| `runtime/minhash_lsh.rs:301` | `as_bytes()` | Cast `&[u32]` → `&[u8]` via `from_raw_parts` | Questionable — alignment assumptions |
| `runtime/minhash_lsh.rs:310` | `from_bytes()` | Cast `&[u8]` → `&[u32]` via `from_raw_parts` | **UNSOUND** — no alignment check, unaligned `u32` read is UB |
| `runtime/minhash_lsh.rs:354` | `get_bytes()` | Same pattern as `as_bytes()` | Questionable |
| `storage/sqlite.rs:127` | `unsafe impl Sync` | Manual Sync on `SqliteTx<'_>` | Risky — requires access pattern audit |
| `storage/sqlite.rs:173` | `std::mem::transmute` | Erases lifetime on SQLite prepared statement | **Unsound by default** — potential use-after-free |
| `storage/rocks.rs:157` | `unsafe impl Sync` | Manual Sync on `RocksDbTx` | Risky |
| `storage/newrocks.rs:129` | `unsafe impl Sync` | Manual Sync on `NewRocksDbTx<'a>` | Risky — required because `rocksdb::Transaction` isn't Sync |

The `minhash_lsh.rs:310` is the most immediately dangerous — a raw `*const u8` to `*const u32` cast
without alignment verification. Fix: replace with `bytemuck::try_cast_slice`.

The `sqlite.rs:173` transmute erases a lifetime, a known source of use-after-free. This goes away
if we strip the sqlite backend (feature-gated, not needed).

The `newrocks.rs:129` unsafe Sync impl is the one we keep — `rocksdb::Transaction` doesn't impl Sync
even though the underlying C++ type is thread-safe. This is arguably correct but needs auditing.

### Unsafe Blocks — graph_builder (42 sites)

| File | Count | Nature |
|------|-------|--------|
| `graph/csr.rs` | 16 | `MaybeUninit`, `from_raw_parts`, CSR construction |
| `compat.rs` | 9 | Polyfills for pre-1.80 stdlib APIs (delete with modern MSRV) |
| `graph_ops.rs` | 5 | `SharedMut` parallel mutable slice access |
| `input/dotgraph.rs` | 4 | Parallel buffer filling |
| `lib.rs` | 3 | `unsafe impl Send/Sync for SharedMut<T>` + `unsafe fn add()` |
| `graph/adj_list.rs` | 2 | Type-punning `Target<NI, ()>` → `NI` |
| `input/graph500.rs` | 2 | Memory-mapped file access |
| `input/edgelist.rs` | 1 | Memory-mapped file creation |

**Most dangerous:** `SharedMut<T>` (`lib.rs`) — a `*mut T` wrapper with manual `Send + Sync` for
parallel mutable writes to disjoint array regions. Safety relies on non-overlapping index invariant
(not compiler-checked). A bug = data race = UB.

**Easy wins:** `compat.rs` (9 sites) — polyfills for `MaybeUninit::write_slice` and `slice::partition_dedup`,
both stable since Rust 1.80. Set modern MSRV and delete the file entirely.

### unwrap() Counts

| Module | Non-test | Test-only | Total |
|--------|----------|-----------|-------|
| `parse/` | 133 | 4 | 137 |
| `runtime/` | 95 | 228 | 323 |
| `storage/` | 85 | 0 | 85 |
| `fts/` | 35 | 32 | 67 |
| `query/` | 59 | 3 | 62 |
| `fixed_rule/` | 31 | 4 | 35 |
| `lib.rs` | 5 | 0 | 5 |
| `utils.rs` | 2 | 0 | 2 |
| **cozo-core total** | **445** | **271** | **716** |
| **graph_builder total** | **19** | **23** | **42** |
| **Combined** | **464** | **294** | **758** |

**Hotspot patterns:**
- `parse/` (133): Almost all `pair.into_inner().next().unwrap()` — structurally guaranteed by PEG grammar, but panics on grammar regression
- `runtime/db.rs` (~35): `lock().unwrap()` on `RwLock`/`Mutex` — panics on lock poisoning
- `storage/` (85): `conn.as_ref().unwrap()`, `pool.lock().unwrap()` — panicking in `Drop` impls is particularly dangerous
- `runtime/hnsw.rs` (~31): `cache.get(key).unwrap()`, `get_int().unwrap()` — assumes cache hits and type correctness

### expect() Counts

| Module | Non-test | Total |
|--------|----------|-------|
| cozo-core | 6 | 6 |
| graph_builder | 13 | 29 |
| **Combined** | **19** | **35** |

### panic!() Outside Tests

**cozo-core: 14 sites**

| File | Line | Context |
|------|------|---------|
| `runtime/hnsw.rs` | 77, 92, 106 | Vector type mismatch in distance computation — **triggerable by user data** |
| `runtime/relation.rs` | 54 | `StoredRelId` overflow guard |
| `query/ra.rs` | 2118, 2121, 2215, 2218 | Invariant violations (joining on reordered/NegJoin) |
| `storage/mod.rs` | 81, 91 | Trait default impls for `par_put`/`par_del` |
| `storage/mem.rs` | 114 | Bare `panic!()` with no message |
| `storage/temp.rs` | 37, 44 | Unsupported operations on temp store |
| `parse/fts.rs` | 82 | Unreachable match arm in parser |

**graph_builder: 1 site** — `input/gdl.rs:30` (type conversion failure in macro).

### Combined Safety Summary

| Metric | cozo-core | graph_builder | Combined |
|--------|-----------|---------------|----------|
| `unsafe` sites | 7 | 42 | **49** |
| `unwrap()` (non-test) | 445 | 19 | **464** |
| `expect()` (non-test) | 6 | 13 | **19** |
| `panic!()` (non-test) | 14 | 1 | **15** |

### Cleanup Effort

**High-priority fixes (UB/unsound):**
1. `minhash_lsh.rs:310` unaligned read → `bytemuck::try_cast_slice` (30 min, goes away with Option B FTS removal)
2. `sqlite.rs:173` transmute → redesign prepared statement caching (1 day, goes away if sqlite backend stripped)
3. `SharedMut` in graph_builder → rayon parallel iterators or crossbeam scoped threads (2-3 days)

**Total estimated effort to reach zero-unsafe, zero-unwrap in library code: 12-20 developer-days.**
Breakdown: cozo-core unsafe (2-3 days), graph_builder unsafe (3-5 days), cozo-core unwrap→Result (5-8 days),
graph_builder unwrap→Result (1-2 days), panic→error (1-2 days).

After stripping FTS (Option B), sqlite, sled, tikv, and legacy rocks, the scope shrinks significantly:
remove 35 fts unwraps, 3 fts unsafes, ~85 storage unwraps, 2 storage unsafes, 1 storage transmute.
**Post-strip estimate: 8-14 developer-days.**

---

## 5. Storage Backend Analysis

### Storage Trait (`storage/mod.rs:31-165`)

Two traits define the key-value abstraction:

```rust
pub trait Storage<'s>: Send + Sync + Clone {
    type Tx: StoreTx<'s>;

    fn storage_kind(&self) -> &'static str;
    fn transact(&'s self, write: bool) -> Result<Self::Tx>;
    fn range_compact(&'s self, lower: &[u8], upper: &[u8]) -> Result<()>;
    fn batch_put<'a>(
        &'a self,
        data: Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> Result<()>;
}

pub trait StoreTx<'s>: Sync {
    fn get(&self, key: &[u8], for_update: bool) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &[u8], val: &[u8]) -> Result<()>;
    fn del(&mut self, key: &[u8]) -> Result<()>;
    fn exists(&self, key: &[u8], for_update: bool) -> Result<bool>;
    fn commit(&mut self) -> Result<()>;
    fn range_scan<'a>(&'a self, lower: &[u8], upper: &[u8])
        -> Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a>;
    fn range_skip_scan_tuple<'a>(&'a self, lower: &[u8], upper: &[u8], valid_at: ValidityTs)
        -> Box<dyn Iterator<Item = Result<Tuple>> + 'a>;
    // ... plus: multi_get, par_put, par_del, del_range_from_persisted,
    //          range_scan_tuple, range_count, total_scan
    // 14 methods total (3 with default impls)
}
```

Pure ordered byte key-value store with MVCC transactional semantics. The `for_update` flag
implements optimistic concurrency (lock key for later commit). The `range_skip_scan_tuple` is
CozoDB's time-travel scan (validity timestamp filtering).

### Backend Inventory

| Backend | File | Lines | Feature Flag | Status |
|---------|------|-------|-------------|--------|
| In-memory | `mem.rs` | 542 | (always compiled) | **Keep** — tests, ephemeral workloads |
| Temp scratch | `temp.rs` | 141 | (always compiled) | **Keep** — query intermediate results |
| New RocksDB | `newrocks.rs` | 560 | `storage-new-rocksdb` | **Keep** — production backend |
| Legacy RocksDB | `rocks.rs` | 527 | `storage-rocksdb` | Strip — uses custom `cozorocks` C++ wrapper |
| SQLite | `sqlite.rs` | 426 | `storage-sqlite` | Strip — we use rusqlite separately |
| Sled | `sled.rs` | 425 | `storage-sled` | Strip — experimental, no time-travel |
| TiKV | `tikv.rs` | 320 | `storage-tikv` | Strip — experimental, no time-travel |

All non-default backends are fully feature-gated via `#[cfg(feature = "...")]` in both `storage/mod.rs`
and `lib.rs`. Stripping them requires zero source edits — just don't enable the feature.

**Total strippable:** 1,698 lines (sqlite + sled + tikv + legacy rocks).

### Backup/Restore Caveat

`runtime/db.rs:645-703` implements backup/restore via SQLite serialization. When `storage-sqlite`
is disabled, these methods degrade to `bail!("backup requires sqlite")`. The fallback stubs already
exist in the code. For Aletheia, we'd implement our own backup mechanism (RocksDB checkpoint or
custom serialization).

### HNSW Vector Index (`runtime/hnsw.rs`, 1035 lines)

**Key finding: HNSW is implemented entirely on top of the `StoreTx` trait.** Vectors are stored
through the same key-value interface as all other data. No backend-specific code.

Architecture:
- HNSW is a set of methods on `SessionTx<'a>`, which holds a type-erased `Box<dyn StoreTx<'a>>`
- All operations use `self.store_tx.put()`, `.get()`, `.del()`, `.exists()`
- The base relation stores data rows including vector fields as `DataValue::Vec(Vector)`
- The HNSW index relation stores graph edges (neighbor links) as byte-encoded KV pairs

Storage layout per HNSW index:
- **Self-links:** `[level, key, idx, subidx, key, idx, subidx]` → `[degree, hash, is_deleted]`
- **Edge links:** `[level, src_key, src_idx, src_subidx, dst_key, dst_idx, dst_subidx]` → `[distance, null, is_deleted]`
- **Canary entry:** `[1, nulls...]` → `[bottom_level, target_key_bytes, false]` (entry point marker)

Key types:
- `HnswIndexManifest` — config: vec_dim, dtype (F32/F64), distance (L2/Cosine/InnerProduct), ef_construction, m/m_max/m_max0
- `VectorCache` — `FxHashMap<CompoundKey, Vector>` for in-memory distance computation during operations
- Distance functions: L2, Cosine, InnerProduct for both F32 and F64 via `ndarray`

Operations:
- `hnsw_put` (line 679) → insert vector, build layer connections
- `hnsw_knn` (line 869) → k-nearest-neighbor search with optional radius/filter
- `hnsw_remove` (line 728) → remove vector and all edges from all layers
- `hnsw_search_level` (line 539) → greedy search at one layer
- `hnsw_select_neighbours_heuristic` (line 470) → neighbor selection

### MinHash LSH (`runtime/minhash_lsh.rs`, 390 lines)

Probabilistic index for approximate Jaccard set similarity. Uses FTS tokenizer for n-gram generation
(see §2 coupling analysis). Stores hash band entries through the same `StoreTx` interface.

**If we adopt Option B (strip FTS + LSH), this goes away entirely.**

---

## 6. Public API Surface

### DbInstance Methods (`lib.rs:106-583`)

| Method | Purpose | Aletheia needs? |
|--------|---------|----------------|
| `new(engine, path, options)` | Create database | **Yes** |
| `run_script(payload, params, mutability)` | Primary query method | **Yes** |
| `run_default(payload)` | Shorthand for `run_script` | Maybe (convenience) |
| `export_relations(relations)` | Export relation data | **Yes** (backup) |
| `import_relations(data)` | Import relation data | **Yes** (restore) |
| `backup_db(path)` | SQLite backup | No (strip sqlite) |
| `restore_backup(path)` | SQLite restore | No (strip sqlite) |
| `register_callback(relation, capacity)` | Watch relation changes | Maybe (event-driven) |
| `register_fixed_rule(name, impl)` | Custom algorithms | **Yes** |
| `multi_transaction(write)` | Multi-statement txn | Maybe |
| `run_script_str/fold_err/etc.` | FFI wrappers (all-string) | No (Rust-native) |
| `new_with_str(engine, path, options)` | String-based constructor | No (FFI) |
| `get_fixed_rules()` | List registered rules | Maybe (introspection) |

### Query Flow

```
run_script(payload, params, mutability)
  → current_validity()                     // timestamp
  → parse_script(payload, &params, ...)    // CozoScript → AST
  → run_script_ast(ast, cur_vld, mutability)
    → Db<S>::run_script_ast(...)           // dispatch per storage engine
      → match ast {
          Sys(op) → handle system ops (create/drop relation, create index, etc.)
          Query(prog) → compile → stratify → magic sets → evaluate → return NamedRows
          Imperative(stmts) → execute imperative program
        }
```

### Publicly Exported Types

| Type | Source | Needed by Aletheia? |
|------|--------|-------------------|
| `DataValue` | `data::value` | **Yes** — query parameters and results |
| `NamedRows` | `runtime::db` | **Yes** — query result container |
| `ScriptMutability` | `runtime::db` | **Yes** — mutable vs immutable queries |
| `FixedRule` | `fixed_rule` | **Yes** — custom algorithm registration |
| `DbInstance` | `lib.rs` | **Yes** — primary entry point |
| `Storage` / `StoreTx` | `storage` | Maybe — if exposing custom backends |
| `Db<S>` | `runtime::db` | Maybe — generic database for custom storage |
| `CallbackOp` | `runtime::callback` | Maybe — relation change watching |
| `ValidityTs` | `data::value` | **Yes** — bi-temporal queries |
| `SourceSpan` | `parse` | No — internal error context |
| `Expr`, `Symbol`, etc. | `data` | No — internal AST types |
| Backend constructors | `storage::*` | No — we use `DbInstance::new()` |

### Proposed Narrowed API for mneme

Based on `crates/mneme/src/knowledge_store.rs` usage patterns:

```rust
// What Aletheia actually needs from cozo-core:
pub struct Db { /* opaque */ }

impl Db {
    pub fn open_rocksdb(path: impl AsRef<Path>) -> Result<Self>;
    pub fn open_mem() -> Result<Self>;
    pub fn run(&self, script: &str, params: BTreeMap<String, DataValue>,
               mutability: ScriptMutability) -> Result<NamedRows>;
    pub fn register_fixed_rule(&self, name: &str, rule: impl FixedRule) -> Result<()>;
    pub fn export_relations(&self, names: &[&str]) -> Result<BTreeMap<String, NamedRows>>;
    pub fn import_relations(&self, data: BTreeMap<String, NamedRows>) -> Result<()>;
}

pub enum DataValue { /* unchanged */ }
pub struct NamedRows { pub headers: Vec<String>, pub rows: Vec<Vec<DataValue>> }
pub enum ScriptMutability { Mutable, Immutable }
pub type ValidityTs = i64;
```

This is **3 types + 1 enum + 6 methods** — down from 30+ exported types and 20+ methods.

---

## 7. Crate Integration Proposal

### Structure Options

#### Option A: Single `mneme-engine` crate

```
crates/
├── mneme/           # knowledge store, recall, embedding (existing)
└── mneme-engine/    # absorbed cozo-core (stripped)
    └── src/
        ├── data/       # core types
        ├── parse/      # CozoScript parser
        ├── query/      # compilation + evaluation
        ├── runtime/    # DB, transactions, HNSW, relations
        ├── storage/    # trait + mem + rocksdb backends
        ├── fixed_rule/ # graph algorithms
        └── lib.rs      # narrowed public API
```

**Pros:** Clean separation, mneme imports mneme-engine as a dependency. Engine evolves independently.
**Cons:** Two crates to maintain. Mneme-engine is still large (~25K lines post-strip).

#### Option B: Integrated into mneme as submodule

```
crates/mneme/
├── src/
│   ├── engine/       # absorbed cozo-core
│   │   ├── data/
│   │   ├── parse/
│   │   ├── query/
│   │   ├── runtime/
│   │   ├── storage/
│   │   └── fixed_rule/
│   ├── knowledge.rs  # existing
│   ├── recall.rs     # existing
│   ├── embedding.rs  # existing
│   └── lib.rs
```

**Pros:** Single crate, simpler dependency graph.
**Cons:** Makes mneme very large. Engine internals visible to all mneme code — harder to enforce boundary.

#### Option C: Split into mneme-datalog + mneme-index (recommended)

```
crates/
├── mneme/            # knowledge store, recall, embedding (existing) — depends on both below
├── mneme-datalog/    # query engine core
│   └── src/
│       ├── data/
│       ├── parse/
│       ├── query/
│       ├── runtime/  # Db, SessionTx, relations (not HNSW)
│       ├── storage/  # trait + backends
│       └── fixed_rule/
└── mneme-index/      # vector + graph indexes
    └── src/
        ├── hnsw.rs   # HNSW implementation
        └── lib.rs    # index management API
```

**Pros:** Separation of concerns matches usage patterns. Datalog engine reusable without vector indexes.
Could later add a `mneme-graph` crate for graph algorithms specifically.
**Cons:** HNSW is currently methods on `SessionTx` — splitting requires introducing a trait boundary or
callback interface between datalog and index modules. More upfront refactoring.

**Recommendation: Option A** (single `mneme-engine`). Aletheia's architecture already has `mneme` as the
memory layer. A single `mneme-engine` crate keeps the boundary clean without over-splitting. The internal
module structure within mneme-engine already provides logical separation. Option C is elegant but the
refactoring cost to decouple HNSW from SessionTx outweighs the architectural benefit at this stage.

### Stripped Line Counts

| Component | Current | After strip | Notes |
|-----------|---------|-------------|-------|
| `data/` | ~11,000 | ~11,000 | Unchanged (leaf module, no FTS types after cleanup) |
| `fts/` | 29,086 | **0** | Removed entirely (Option B) |
| `runtime/` | 7,569 | ~7,000 | Remove LSH (390), FTS refs (~180) |
| `query/` | 6,830 | ~6,500 | Remove FTS RA variant, match arms (~330) |
| `fixed_rule/` | 4,413 | ~4,000 | Remove CSV (215) + JSON (186) readers |
| `parse/` | 3,189 | ~2,900 | Remove `parse/fts.rs` (164), FTS grammar (~125) |
| `storage/` | 3,106 | ~1,400 | Keep only mod.rs (165) + mem (542) + temp (141) + newrocks (560) |
| `lib.rs` | 664 | ~300 | Strip FFI wrappers, DbInstance enum → single backend |
| `utils.rs` | 31 | 31 | Unchanged |
| **Total** | **~66,163** | **~33,131** | **~50% reduction** |

After stripping FTS, LSH, CSV/JSON import, and unused storage backends: **~33K lines** remain.
The `data/` module (~11K) is the largest retained component and has the lowest unsafe/unwrap density.

### Standards Alignment Effort

| Standard | Gap | Effort |
|----------|-----|--------|
| `snafu` errors (not `miette`) | All errors use `miette::bail!` | 3-5 days — rewrite error handling across all modules |
| `#[instrument]` on public fns | Zero tracing instrumentation | 2-3 days — add `tracing` spans to key paths |
| Zero `unwrap()` in library code | 464 non-test unwraps (post-strip: ~340) | 5-8 days — tedious but mechanical |
| Zero `unsafe` in library code | 7 cozo-core + 42 graph_builder (post-strip: 4 + 42) | 5-8 days — some require architectural changes |
| Zero `panic!()` | 15 sites (post-strip: ~10) | 1-2 days |
| `.js` import extensions | N/A (Rust) | 0 |
| Typed error hierarchy | `miette` → `snafu` with `AletheiaError` | Part of snafu migration |
| No `any` / `unknown` | Multiple `Box<dyn Any>` in data types | 2-3 days — needs careful refactoring |
| **Total** | | **18-29 developer-days** |

### Phased Absorption Plan

#### Phase 1: Strip and Compile (1-2 days)

1. Copy vendored source into `crates/mneme-engine/src/`
2. Restore `data/` module from upstream CozoDB 0.7.6 source
3. Strip FTS + LSH (Option B): delete `fts/`, `parse/fts.rs`, `minhash_lsh.rs`; edit 15 files
4. Strip unused storage backends: disable features
5. Strip CSV/JSON readers: remove 2 files, 2 registration entries
6. Strip FFI/string wrappers from `lib.rs`
7. Narrow public API to proposed surface
8. Verify: `cargo check -p mneme-engine`
9. **Gate:** Clean compilation with zero warnings

#### Phase 2: Critical Safety Fixes (2-3 days)

1. Fix UB: `minhash_lsh.rs` alignment → goes away with FTS strip
2. Fix UB: `sqlite.rs` transmute → goes away with sqlite strip
3. Audit `newrocks.rs:129` unsafe Sync impl — document or redesign
4. Replace `SharedMut` in graph_builder with safe alternatives
5. Delete `compat.rs` (set MSRV to 1.80+)
6. **Gate:** Zero unsound unsafe remaining

#### Phase 3: Error Migration (3-5 days)

1. Replace `miette::bail!` with `snafu` error enums per module
2. Define `MnemeEngineError` hierarchy following `AletheiaError` pattern
3. Replace `unwrap()` in storage module (highest crash risk)
4. Replace `panic!()` in storage trait defaults with `Result`
5. **Gate:** `cargo clippy` clean, zero panics in non-test code

#### Phase 4: unwrap() Elimination (5-8 days)

1. `parse/` (133 sites): Helper functions wrapping PEG output extraction
2. `runtime/` (95 sites): Lock handling, HNSW cache access
3. `query/` (59 sites): Mixed assertion/access patterns
4. `fixed_rule/` (31 sites): Algorithm assumptions
5. `graph_builder` (19 sites): Lock guards, slice access
6. **Gate:** Zero non-test `unwrap()` calls

#### Phase 5: Instrumentation (2-3 days)

1. Add `#[instrument]` to public API methods
2. Add `tracing` spans to query compilation and evaluation
3. Add structured logging to HNSW operations
4. **Gate:** Key operations visible in traces

#### Phase 6: Integration (1-2 days)

1. Wire `mneme-engine` into `mneme` crate
2. Replace knowledge_store.rs templates with live CozoDB calls
3. Integration tests: schema creation, CRUD, HNSW search, graph queries
4. **Gate:** All mneme integration tests pass

**Total: 14-23 developer-days across 6 phases.** Phases 1-2 ship first (functional engine).
Phase 3-4 can be done incrementally alongside feature work. Phase 5-6 complete the integration.

---

## 8. graph_builder Assessment

### What cozo-core Uses

cozo-core does **not** import `graph_builder` directly. The dependency chain:

```
cozo-core → graph 0.3.1 (optional, feature "graph-algo") → graph_builder 0.4.1
```

Types used (via `graph` crate re-exports):
- `DirectedCsrGraph<u32, (), f64>` — CSR graph data structure
- `GraphBuilder` — graph construction from edge lists
- `CsrLayout` — CSR storage layout control
- `Graph` trait — `node_count()`, `edge_count()`
- `DirectedNeighbors` / `DirectedNeighborsWithValues` traits — neighbor iteration
- `page_rank`, `PageRankConfig` — from `graph` crate (not graph_builder)

Used by 12 of 17 algorithms in `fixed_rule/algos/`. Pattern:

```rust
// Build CSR graph from Datalog edge relation
let graph: DirectedCsrGraph<u32, (), f64> = GraphBuilder::new()
    .csr_layout(CsrLayout::Sorted)
    .edges_with_values(edges)
    .build();

// Traverse
for Target { target, value } in graph.out_neighbors_with_values(node) { ... }
```

### Size and Usage

| Metric | Value |
|--------|-------|
| Total lines | 5,927 |
| Lines actually exercised by cozo-core | ~1,800 |
| Lines unused by cozo-core | ~4,100 |

Unused components:
- `adj_list.rs` (1,065 lines) — `UndirectedCsrGraph`, `AdjacencyListGraph` never used
- `input/dotgraph.rs` (625 lines) — DOT format, feature-gated, unused
- `input/gdl.rs` (208 lines) — GDL format, unused
- `input/graph500.rs` (127 lines) — benchmark format, unused
- `compat.rs` (186 lines) — pre-1.80 polyfills, delete with modern MSRV
- `graph_ops.rs` (775 lines) — mostly parallel ops, relabel, serialize — largely unused

### petgraph Replacement?

**Possible but questionable value.** Costs:
- Rewrite 12 algorithm files to petgraph's API
- petgraph's CSR construction is single-threaded (graph_builder uses rayon)
- `graph` crate provides PageRank; petgraph doesn't — would need custom implementation
- The `graph` crate wraps `graph_builder` — replacing one means replacing both

**Recommendation: Absorb both `graph` and `graph_builder` alongside cozo-core.** Strip unused
components (~4,100 lines), leaving ~1,800 lines of graph_builder that are well-integrated with the
algorithm implementations. The total unsafe cost (42 sites) is concentrated in code that can be
incrementally modernized (delete compat.rs: -9, safe parallel patterns: -8, modern MaybeUninit: -16).

### Feature-Gate Note

If Aletheia doesn't need graph algorithms immediately, the entire `graph-algo` feature (and thus
graph_builder) can be disabled at compile time. The knowledge store's basic Datalog queries, HNSW
vector search, and relation management work without it. Graph algorithms can be enabled later when
needed for PageRank entity importance, community detection, or shortest-path reasoning chains.

---

## Appendix: File-Level Line Counts

### cozo-core/src/ (vendored, excludes data/)

```
fts/tokenizer/stop_word_filter/stopwords.rs  21,885
fts/tokenizer/ascii_folding_filter.rs         4,047
query/ra.rs                                   2,398
runtime/db.rs                                 1,969
runtime/tests.rs                              1,614
runtime/relation.rs                           1,473
query/stored.rs                               1,229
parse/query.rs                                1,102
runtime/hnsw.rs                               1,035
fixed_rule/mod.rs                               920
parse/sys.rs                                    676
query/eval.rs                                   670
query/compile.rs                                665
lib.rs                                          664
query/magic.rs                                  659
storage/newrocks.rs                             560
storage/mem.rs                                  542
storage/rocks.rs                                527
storage/sqlite.rs                               426
storage/sled.rs                                 425
runtime/minhash_lsh.rs                          390
query/reorder.rs                                377
storage/tikv.rs                                 320
fixed_rule/algos/shortest_path_dijkstra.rs      432
fixed_rule/algos/louvain.rs                     318
parse/expr.rs                                   513
(remaining 60 files)                          8,976
─────────────────────────────────────────────
Total                                        54,888
```

### graph_builder/src/

```
graph/csr.rs          1,256
graph/adj_list.rs     1,065
graph_ops.rs            775
input/dotgraph.rs       625
builder.rs              540
lib.rs                  476
input/edgelist.rs       347
input/gdl.rs            208
compat.rs               186
storage/temp.rs         141
input/graph500.rs       127
index.rs                103
input/mod.rs            107
input/binary.rs          38
prelude.rs               38
graph/mod.rs             36
─────────────────────
Total                 5,927
```
