# Krites Clean-Room Rewrite: API Design

## Context

Replacing 55K LOC vendored CozoDB with a purpose-built Datalog engine.
Research inventories identified: 133 query instances, 8 relations, 24 graph
algorithms, FTS (BM25) + HNSW vector search, bi-temporal queries.

Decision: full ownership of every LOC. Keep all 24 graph algorithms.
Zero unsafe. Zero blanket suppressions. eidos-native types at boundary.

## Consumer API Contract

All consumers (episteme, graphe migration, etc.) call:

```rust
db.run(script: &str, params: BTreeMap<String, DataValue>, mutability: ScriptMutability) -> Result<NamedRows>
db.run_read_only(script: &str, params: BTreeMap<String, DataValue>) -> Result<NamedRows>
```

This interface MUST be preserved for backward compatibility. The internals change; the boundary stays.

## Public API Surface

### Db (facade)

```rust
pub struct Db { /* opaque */ }

impl Db {
    // Construction
    pub fn open_mem() -> Result<Self>;
    pub fn open_fjall(path: impl AsRef<Path>) -> Result<Self>;
    pub fn with_cache(self, capacity: NonZeroUsize) -> Self;

    // Query execution
    pub fn run(&self, script: &str, params: BTreeMap<String, Value>, mutability: Mutability) -> Result<Rows>;
    pub fn run_read_only(&self, script: &str, params: BTreeMap<String, Value>) -> Result<Rows>;

    // Transactions
    pub fn transaction(&self, write: bool) -> Transaction;

    // Backup & restore
    pub fn backup(&self, path: impl AsRef<Path>) -> Result<()>;
    pub fn restore(&self, path: impl AsRef<Path>) -> Result<()>;

    // Relation export/import
    pub fn export_relations(&self, names: &[&str]) -> Result<BTreeMap<String, Rows>>;
    pub fn import_relations(&self, data: BTreeMap<String, Rows>) -> Result<()>;

    // Fixed rules (graph algorithms)
    pub fn register_fixed_rule<R: FixedRule + 'static>(&self, name: &str, rule: R);

    // Cache
    pub fn cache_stats(&self) -> Option<CacheStats>;
}
```

### Value (replaces DataValue)

```rust
/// Runtime value in the Datalog engine.
///
/// Maps to eidos types at the API boundary:
/// - Str → String fields in Fact/Entity/Relationship
/// - Float → confidence, scores
/// - Int → counts, timestamps
/// - Bool → is_forgotten, flags
/// - Vector → embedding vectors
/// - Null → optional fields
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Arc<str>),
    Bytes(Arc<[u8]>),
    List(Arc<[Value]>),
    Vector(Vector),
    Timestamp(jiff::Timestamp),
}

pub enum Vector {
    F32(Arc<[f32]>),
    F64(Arc<[f64]>),
}
```

### Rows (replaces NamedRows)

```rust
/// Query result with column headers and typed rows.
pub struct Rows {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}
```

### Mutability (replaces ScriptMutability)

```rust
pub enum Mutability {
    Immutable,
    Mutable,
}
```

## Internal Architecture

### Module Breakdown

```
src/
├── lib.rs              # Db facade, public re-exports
├── value.rs            # Value enum, Vector, type conversions
├── rows.rs             # Rows (query results)
├── error.rs            # snafu error types
├── parse/
│   ├── mod.rs          # Datalog parser entry point
│   ├── lexer.rs        # Token scanner
│   ├── ast.rs          # AST types (Query, Rule, Atom, Expr)
│   └── span.rs         # Source spans for error reporting
├── plan/
│   ├── mod.rs          # Query planner
│   ├── normalize.rs    # AST → normalized rules
│   ├── stratify.rs     # Stratification (negation safety)
│   └── optimize.rs     # Join ordering, filter pushdown
├── eval/
│   ├── mod.rs          # Evaluation engine
│   ├── semi_naive.rs   # Semi-naive bottom-up evaluation
│   ├── aggregation.rs  # count, sum, max, min, mean
│   └── temporal.rs     # Bi-temporal filter evaluation
├── storage/
│   ├── mod.rs          # Storage trait
│   ├── mem.rs          # In-memory backend
│   ├── fjall.rs        # fjall LSM-tree backend
│   └── tx.rs           # Transaction abstraction
├── index/
│   ├── mod.rs          # Index trait
│   ├── hnsw.rs         # HNSW vector index (Cosine, F32)
│   ├── fts.rs          # Full-text search (BM25, stemming)
│   └── btree.rs        # B-tree relational index
├── schema/
│   ├── mod.rs          # Schema DDL (`:create`, `:replace`)
│   ├── relation.rs     # Relation definitions, column types
│   └── validation.rs   # Type checking, constraint enforcement
├── algo/
│   ├── mod.rs          # FixedRule trait + registry
│   ├── pagerank.rs     # PageRank
│   ├── community.rs    # Louvain community detection
│   ├── path.rs         # BFS, Dijkstra, A*, K-shortest
│   ├── centrality.rs   # Degree, Closeness, Betweenness
│   ├── traversal.rs    # DFS, BFS, RandomWalk
│   ├── spanning.rs     # Prim, Kruskal
│   ├── connectivity.rs # Connected/Strongly-connected components
│   └── clustering.rs   # Label propagation, K-core, coefficients
├── cache.rs            # LRU query cache
└── string_ops.rs       # contains, starts_with, str_includes
```

### Storage Trait

```rust
pub trait Storage: Send + Sync + 'static {
    type Transaction: StorageTx;
    fn begin(&self, write: bool) -> Result<Self::Transaction>;
}

pub trait StorageTx: Send {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&mut self, key: &[u8]) -> Result<()>;
    fn scan_prefix(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + '_>;
    fn commit(self) -> Result<()>;
    fn rollback(self) -> Result<()>;
}
```

### FixedRule Trait (graph algorithms)

```rust
pub trait FixedRule: Send + Sync {
    fn arity(&self, options: &BTreeMap<String, Value>) -> Result<usize>;
    fn run(&self, payload: FixedRulePayload) -> Result<Rows>;
}
```

All 24 graph algorithms implement this trait. Invoked via `<~` syntax in Datalog.

### Index Trait

```rust
pub trait Index: Send + Sync {
    fn insert(&self, id: &[u8], value: &Value) -> Result<()>;
    fn delete(&self, id: &[u8]) -> Result<()>;
    fn search(&self, query: &Value, k: usize, params: &BTreeMap<String, Value>) -> Result<Vec<(Vec<u8>, f64)>>;
}
```

HNSW and FTS implement this. Invoked via `~relation:index_name{}` syntax.

## Evaluation Strategy

Semi-naive bottom-up evaluation (same as CozoDB):
1. Parse script → AST
2. Normalize → stratified rules
3. Plan → join ordering, filter pushdown
4. Evaluate → iterate to fixpoint per stratum
5. Aggregate → apply count/sum/max/min/mean
6. Temporal filter → apply valid_from/valid_to bounds
7. Order/limit → sort and truncate
8. Return Rows

## Migration Path

Feature flag `krites-v2` during development:
- Default: existing CozoDB engine (current behavior)
- `krites-v2`: new clean-room engine
- Both share `Db` facade API  -  consumers don't change
- Validation: run all 133 episteme queries on both engines, compare results
- When parity confirmed: make `krites-v2` default, then remove CozoDB

## Estimated Scope

| Module | Lines | Complexity |
|--------|-------|------------|
| Value + Rows | 500 | Low |
| Parser | 2,000 | Medium |
| Planner | 1,500 | Medium |
| Evaluator | 3,000 | High |
| Storage (mem + fjall) | 1,500 | Medium |
| HNSW index | 2,000 | High |
| FTS/BM25 index | 1,500 | Medium |
| Schema | 1,000 | Low |
| 24 graph algorithms | 5,000 | Medium (each ~200 LOC) |
| Cache + string ops | 500 | Low |
| Error types + tests | 2,000 | Low |
| **Total** | **~20,500** | |

Within the 25-35K LOC target. Tests will add ~5-10K LOC.
