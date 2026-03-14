# Mneme Crate Split Plan

> Research document for issue #1214. Proposes decomposing `aletheia-mneme`
> (105K lines, 185 files) into focused crates with clearer module boundaries
> and faster incremental compiles.

---

## Current state

| Component | Lines | Files | Feature gate |
|-----------|------:|------:|--------------|
| Engine (vendored CozoDB) | 68,829 | 118 | `mneme-engine` |
| Session store (SQLite) | 1,966 | 4 | `sqlite` |
| Knowledge store (Datalog) | 6,917 | 8 | `mneme-engine` |
| Knowledge types + pipelines | ~9,500 | 16 | always / mixed |
| Embedding / HNSW | ~860 | 2 | `embed-candle` / `hnsw_rs` |
| Import / Export / Portability | ~1,280 | 3 | `sqlite` |
| Backup / Migration / Retention | ~1,670 | 3 | `sqlite` |
| Recall + Graph intelligence | ~940 | 2 | always |
| Skills (parsing + extraction) | ~2,850 | 5+ | always |
| Misc (types, id, error, vocab) | ~1,520 | 5 | always |
| Test files | 18,370 | 23 | — |
| **Total** | **~104,828** | **185** | — |

The engine alone is 66% of the crate. A single-line change in `types.rs`
forces recompilation of everything including the engine, because `cargo`
operates at crate granularity.

---

## Cross-module dependency map

```
                          ┌──────────────┐
                          │   types.rs   │  (no internal deps)
                          │   id.rs      │
                          │   error.rs   │
                          │   vocab.rs   │
                          └──────┬───────┘
                                 │
              ┌──────────────────┼──────────────────┐
              ▼                  ▼                  ▼
    ┌─────────────────┐  ┌─────────────┐  ┌──────────────────┐
    │  SESSION GROUP   │  │  KNOWLEDGE  │  │     ENGINE       │
    │                  │  │   TYPES     │  │  (vendored cozo) │
    │  store/          │  │             │  │                  │
    │  migration.rs    │  │ knowledge.rs│  │  data/           │
    │  schema.rs       │  │ embedding.rs│  │  query/          │
    │  backup.rs       │  │ hnsw_index. │  │  parse/          │
    │  retention.rs    │  │             │  │  runtime/        │
    │  import.rs ──────┼──│─►           │  │  storage/        │
    │  export.rs ──────┼──│─►           │  │  fts/            │
    │  portability.rs  │  │             │  │  fixed_rule/     │
    │                  │  │             │  │  utils            │
    │  deps: rusqlite  │  │ deps: serde │  │                  │
    │        jiff      │  │       jiff  │  │ deps: ndarray    │
    │        snafu     │  │       snafu │  │       rayon      │
    │                  │  │             │  │       crossbeam   │
    └─────────────────┘  └──────┬──────┘  │       pest        │
                                │         │       fjall (opt)  │
                                ▼         │       +15 more     │
                       ┌────────────────┐ │                  │
                       │  KNOWLEDGE     │ │                  │
                       │  STORE         │ │                  │
                       │                │ │                  │
                       │ knowledge_     ├─┤─►uses engine::Db │
                       │   store/       │ │                  │
                       │ query.rs       │ │                  │
                       │ recall.rs      │ │                  │
                       │ graph_intel.rs │ │                  │
                       │ conflict.rs    │ │                  │
                       │ dedup.rs       │ │                  │
                       │ succession.rs  │ │                  │
                       │ instinct.rs    │ │                  │
                       │ skill.rs       │ │                  │
                       │ skills/        │ │                  │
                       │ extract/       │ │                  │
                       │ consolidation/ │ │                  │
                       │ query_rewrite  │ │                  │
                       └────────────────┘ └──────────────────┘
```

### Detailed import graph (crate-internal `use crate::` references)

| Source module | Depends on |
|---------------|------------|
| `types` | — |
| `id` | — |
| `error` | — |
| `vocab` | — |
| `schema` | — |
| `knowledge` | `id` |
| `embedding` | — |
| `skill` | — |
| `instinct` | — |
| `query_rewrite` | — |
| `store/` | `error`, `migration`, `types` |
| `migration` | `error`, `schema` |
| `backup` | `error` |
| `retention` | `error` |
| `portability` | `knowledge` (feature-gated) |
| `import` | `error`, `portability`, `store`, `schema` |
| `export` | `error`, `portability`, `store`, `types`, `knowledge`, `id` (feature-gated) |
| `recall` | `knowledge`, `graph_intelligence` |
| `graph_intelligence` | `knowledge_store`, `succession`, `id` |
| `succession` | `id`, `knowledge`, `recall` |
| `conflict` | `id`, `knowledge`, `knowledge_store`, `engine` (feature-gated) |
| `dedup` | `id` |
| `consolidation/` | `id` |
| `extract/` | — |
| `skills/` | — |
| `knowledge_store/` | `engine`, `query`, `knowledge`, `error`, `id`, `embedding` |
| `query` | `engine` |
| `hnsw_index` | `error` |

### Downstream consumers (5 crates)

| Crate | Session store | Knowledge store | Engine | Recall | Embedding | Skills | Types |
|-------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| aletheia (binary) | x | x | x | x | x | x | — |
| nous | x | x | — | x | x | x | x |
| pylon | x | x | — | — | — | — | x |
| diaporeia | x | — | — | — | — | — | — |
| integration-tests | x | x | x | x | x | — | x |

---

## Proposed split

### Four new crates

| Crate | Contents | Lines (approx) | Key dependencies |
|-------|----------|---------------:|-----------------|
| **mneme-types** | `types.rs`, `id.rs`, `error.rs`, `vocab.rs`, `knowledge.rs`, `embedding.rs` (trait only) | ~2,400 | serde, jiff, snafu, ulid |
| **mneme-engine** | `engine/` (entire vendored CozoDB tree) | ~68,800 | ndarray, rayon, crossbeam, pest, fjall (opt), +15 |
| **mneme-session** | `store/`, `schema.rs`, `migration.rs`, `backup.rs`, `retention.rs`, `portability.rs`, `import.rs`, `export.rs` | ~5,200 | rusqlite, mneme-types |
| **mneme-knowledge** | `knowledge_store/`, `query.rs`, `recall.rs`, `graph_intelligence.rs`, `conflict.rs`, `dedup.rs`, `succession.rs`, `instinct.rs`, `skill.rs`, `skills/`, `extract/`, `consolidation/`, `query_rewrite.rs`, `hnsw_index.rs`, embedding impls | ~23,700 | mneme-types, mneme-engine, hnsw_rs (opt), strsim |

Plus the existing **aletheia-mneme** crate becomes a thin facade re-exporting
from the four sub-crates, so downstream `use aletheia_mneme::*` continues to
compile unchanged.

### Dependency graph after split

```
mneme-types          (leaf — no workspace deps)
    ▲       ▲
    │       │
mneme-session   mneme-engine       (independent of each other)
    ▲               ▲
    │               │
    └───┬───────────┘
        │
  mneme-knowledge                  (depends on types + engine)
        ▲
        │
  aletheia-mneme                   (facade: re-exports all four)
```

`mneme-session` and `mneme-engine` have **no dependency on each other**.
This is the key property that enables parallel compilation and independent
iteration.

---

## Public API surface per crate

### mneme-types

```rust
// Domain types
pub struct Session { ... }
pub struct Message { ... }
pub struct UsageRecord { ... }
pub struct BlackboardRow { ... }
pub struct AgentNote { ... }
pub enum SessionStatus { Active, Archived, Distilled }
pub enum SessionType { Primary, Background, Ephemeral }
pub enum Role { System, User, Assistant, ToolResult }

// Knowledge types
pub struct Fact { ... }
pub struct Entity { ... }
pub struct Relationship { ... }
pub struct EmbeddedChunk { ... }
pub struct RecallResult { ... }
pub enum EpistemicTier { Verified, Inferred, Assumed }
pub enum FactType { Identity, Preference, Skill, Relationship, Event, Task, Observation }
pub enum ForgetReason { ... }

// IDs
pub struct FactId(CompactString);
pub struct EntityId(CompactString);
pub struct EmbeddingId(CompactString);

// Embedding trait
pub trait EmbeddingProvider: Send + Sync { ... }

// Vocabulary
pub fn normalize_relation(raw: &str) -> RelationType;

// Error
pub enum Error { ... }
pub type Result<T> = std::result::Result<T, Error>;
```

### mneme-engine

```rust
// Database facade
pub enum Db { Mem(...), Fjall(...) }
pub fn Db::open_mem() -> Result<Self>;
pub fn Db::open_fjall(path) -> Result<Self>;
pub fn Db::run(script, params, mutability) -> Result<NamedRows>;
pub fn Db::run_read_only(script, params) -> Result<NamedRows>;

// Core types
pub struct NamedRows { pub headers: Vec<String>, pub rows: Vec<Vec<DataValue>> }
pub enum DataValue { ... }  // Null, Bool, Int, Float, Str, Bytes, List, Vec, ...
pub type Vector = ndarray::Array1<f32>;
pub enum ScriptMutability { Mutable, Immutable }

// Extensibility
pub trait FixedRule: Send + Sync { ... }
pub fn Db::register_fixed_rule(name, rule) -> Result<()>;
pub fn Db::register_callback(relation, capacity) -> (u32, Receiver<...>);

// Transactions
pub struct MultiTransaction { pub sender, pub receiver }
pub fn Db::multi_transaction(write: bool) -> MultiTransaction;

// Storage backends
pub struct MemStorage;
pub struct FjallStorage;  // feature = "storage-fjall"
```

### mneme-session

```rust
// Session store
pub struct SessionStore { ... }
pub fn SessionStore::open(path) -> Result<Self>;
pub fn SessionStore::open_in_memory() -> Result<Self>;

// Session CRUD
pub fn create_session(...) -> Result<Session>;
pub fn find_session(...) -> Result<Option<Session>>;
pub fn find_or_create_session(...) -> Result<Session>;
pub fn list_sessions(...) -> Result<Vec<Session>>;
pub fn update_session_status(...) -> Result<()>;

// Messages
pub fn append_message(...) -> Result<i64>;
pub fn get_history(...) -> Result<Vec<Message>>;
pub fn record_usage(...) -> Result<()>;

// Peripherals
pub fn add_note(...) -> Result<String>;
pub fn blackboard_write(...) -> Result<()>;

// Backup
pub struct BackupManager<'a> { ... }
pub fn BackupManager::create_backup() -> Result<BackupResult>;

// Retention
pub struct RetentionPolicy { ... }
pub fn RetentionPolicy::apply(conn) -> Result<RetentionResult>;

// Migration
pub fn run_migrations(conn) -> Result<MigrationResult>;

// Import / Export
pub fn import_agent(store, file, options) -> Result<ImportResult>;
pub fn export_agent(store, options) -> Result<AgentFile>;

// Portability
pub struct AgentFile { ... }
pub struct ExportedSession { ... }
```

### mneme-knowledge

```rust
// Knowledge store
pub struct KnowledgeStore { ... }
pub struct KnowledgeConfig { ... }
pub fn KnowledgeStore::open_mem() -> Result<Arc<Self>>;
pub fn KnowledgeStore::open_fjall(path, config) -> Result<Arc<Self>>;

// Fact CRUD
pub fn store_fact(...) -> Result<()>;
pub fn get_fact(...) -> Result<Option<Fact>>;
pub fn forget_fact(...) -> Result<()>;

// Search
pub struct HybridQuery { ... }
pub struct HybridResult { ... }
pub fn hybrid_search(query) -> Result<Vec<HybridResult>>;

// Recall scoring
pub struct RecallEngine { ... }
pub struct RecallWeights { ... }
pub struct ScoredResult { ... }
pub struct FactorScores { ... }

// Graph intelligence
pub struct GraphContext { ... }
pub fn build_graph_context() -> Result<GraphContext>;

// Conflict detection
pub fn detect_conflicts(facts, store, nous_id, classifier) -> Result<BatchConflictResult>;

// Entity dedup
pub fn generate_candidates(entities, similarities) -> Vec<EntityMergeCandidate>;

// Extraction
pub struct Extraction { ... }
pub trait ExtractionProvider { ... }

// Instinct
pub struct ToolObservation { ... }
pub fn aggregate_observations(obs) -> Vec<BehavioralPattern>;

// Skills
pub struct SkillContent { ... }
pub fn parse_skill_md(source, slug) -> Result<SkillContent>;
pub trait SkillExtractionProvider { ... }

// Succession
pub struct DomainVolatility { ... }
pub fn compute_volatility(...) -> f64;

// Query rewrite
pub trait RewriteProvider { ... }
pub struct QueryRewriter { ... }

// Embedding implementations
pub struct MockEmbeddingProvider { ... }
pub struct CandelProvider { ... }  // feature = "embed-candle"
pub fn create_provider(config) -> Result<Box<dyn EmbeddingProvider>>;

// HNSW
pub struct HnswIndex { ... }  // feature = "hnsw_rs"
```

---

## Incremental compile time impact

### Current situation

Any change to any `.rs` file under `crates/mneme/src/` invalidates the entire
crate. Cargo recompiles all 105K lines as one compilation unit. The engine's
68K lines of vendored CozoDB (heavy generics, proc macros from `pest_derive`,
`serde_derive` on 50+ types) dominate compile time.

Typical full-crate recompile: **45–90 seconds** depending on machine (debug
mode, incremental enabled). Even a one-line change to `types.rs` pays this
cost.

### After split (estimated)

| Change location | Crates recompiled | Lines recompiled | Estimated time |
|-----------------|:-----------------:|:----------------:|:--------------:|
| `mneme-types` | types + session + knowledge + facade | ~100K | ~50–80s (similar to today, but parallelized) |
| `mneme-session` | session + facade | ~5K | ~3–5s |
| `mneme-engine` | engine + knowledge + facade | ~93K | ~45–70s |
| `mneme-knowledge` | knowledge + facade | ~24K | ~10–15s |
| `mneme-session` only (no type change) | session + facade | ~5K | **~3–5s** (vs 45–90s today) |
| `mneme-knowledge` only (no type change) | knowledge + facade | ~24K | **~10–15s** (vs 45–90s today) |

**Key wins:**
- Session store changes (the most frequent during feature work) drop from
  45–90s to 3–5s — **10–20x faster**.
- Knowledge pipeline changes (recall tuning, extraction prompts) drop from
  45–90s to 10–15s — **4–6x faster**.
- Engine changes remain expensive but are rare (vendored, stable code).
- `mneme-session` and `mneme-engine` compile **in parallel** since they're
  independent — the critical path shortens.

### Why this works

The engine is vendored and rarely changes. Today, every mneme edit pays the
engine tax. After splitting, only changes to `mneme-engine` or `mneme-types`
trigger engine recompilation. Session and knowledge work — the common case —
avoids it entirely.

---

## Migration plan

### Phase 1: Extract `mneme-types` (lowest risk, highest prerequisite value)

**What moves:** `types.rs`, `id.rs`, `error.rs`, `vocab.rs`, `knowledge.rs`,
`EmbeddingProvider` trait from `embedding.rs`.

**Why first:** Every other crate depends on these types. Extracting them
creates the shared foundation. Zero behavioral change — these are pure data
types with `Serialize`/`Deserialize`.

**Steps:**
1. Create `crates/mneme-types/` with `Cargo.toml` (deps: serde, jiff, snafu, ulid, compact_str).
2. Move type files. Update `use crate::` → `use mneme_types::`.
3. Add `mneme-types` as dependency to `aletheia-mneme`.
4. Re-export from `aletheia-mneme::*` for backward compatibility.
5. Verify: `cargo test --workspace`.

**Risk:** Low. Pure data types, no runtime behavior.

### Phase 2: Extract `mneme-engine` (largest code mass, cleanest boundary)

**What moves:** The entire `engine/` directory (68K lines).

**Why second:** The engine is already feature-gated behind `mneme-engine` and
has a narrow public API (`Db`, `DataValue`, `NamedRows`, `FixedRule`). The
`#[expect(...)]` lint suppressions on vendored modules already document the
boundary. This is the highest-value split for compile times.

**Steps:**
1. Create `crates/mneme-engine/` with `Cargo.toml` (deps: ndarray, rayon,
   crossbeam, pest, fjall (opt), etc.).
2. Move `engine/` directory wholesale. The `pub(crate)` items become `pub`
   within the new crate, with `mod.rs` re-exporting the narrow public API.
3. Add `mneme-engine` as dependency to `aletheia-mneme`.
4. Update `knowledge_store/` imports: `crate::engine::` → `mneme_engine::`.
5. Re-export `Db`, `DataValue`, `NamedRows` etc. from facade.
6. Verify: `cargo test --workspace`.

**Risk:** Medium. Internal engine types currently use `pub(crate)` visibility
and are accessed by `knowledge_store/` via `crate::engine::`. These become
cross-crate imports and may require visibility adjustments. The engine's lint
`#[expect]` attributes may need scope changes.

### Phase 3: Extract `mneme-session` (clean SQLite boundary)

**What moves:** `store/`, `schema.rs`, `migration.rs`, `backup.rs`,
`retention.rs`, `portability.rs`, `import.rs`, `export.rs`.

**Why third:** Session store depends only on `mneme-types`. No engine
dependency (except `import_knowledge` which is feature-gated and can stay in
the facade or move to `mneme-knowledge`). Extracting after types are stable
avoids churn.

**Steps:**
1. Create `crates/mneme-session/` with `Cargo.toml` (deps: rusqlite,
   mneme-types, jiff, snafu, tracing, ulid, serde_json).
2. Move session files. Update imports.
3. Move `import_knowledge()` to `mneme-knowledge` (it depends on the engine).
4. Re-export `SessionStore` etc. from facade.
5. Verify: `cargo test --workspace`.

**Risk:** Low. Clean boundary already exists via feature gating. The only
cross-boundary concern is `import.rs`/`export.rs` which reference
`portability.rs` knowledge types under feature gates — these feature-gated
paths move to `mneme-knowledge`.

### Phase 4: Extract `mneme-knowledge` (remaining non-engine, non-session code)

**What moves:** `knowledge_store/`, `query.rs`, `recall.rs`,
`graph_intelligence.rs`, `conflict.rs`, `dedup.rs`, `succession.rs`,
`instinct.rs`, `skill.rs`, `skills/`, `extract/`, `consolidation/`,
`query_rewrite.rs`, `hnsw_index.rs`, embedding impls.

**Why last:** This has the most internal cross-references (recall → knowledge,
graph_intelligence → succession → knowledge_store). Waiting until types and
engine are stable makes the extraction mechanical.

**Steps:**
1. Create `crates/mneme-knowledge/` with `Cargo.toml` (deps: mneme-types,
   mneme-engine, strsim, hnsw_rs (opt), serde, snafu, tracing, jiff).
2. Move files. Update imports throughout.
3. Move feature-gated knowledge export/import paths from `mneme-session`.
4. Re-export from facade.
5. Verify: `cargo test --workspace`.

**Risk:** Medium. Dense internal dependency graph between recall, graph
intelligence, succession, and knowledge_store. Requires careful ordering of
imports. Some circular reference risk between `graph_intelligence` (which adds
methods to `KnowledgeStore`) and `knowledge_store` — may need a trait
extraction or move the graph methods into `knowledge_store` module directly.

### Phase 5: Thin facade

**What remains in `aletheia-mneme`:** A `lib.rs` that re-exports from the
four sub-crates. Downstream code continues to `use aletheia_mneme::*`
unchanged.

```rust
// crates/mneme/src/lib.rs (after split)
pub use mneme_types::*;
pub use mneme_session::*;
pub use mneme_engine::{Db, DataValue, NamedRows, ...};
pub use mneme_knowledge::*;
```

Downstream crates can later migrate to direct dependencies on the sub-crate
they actually need, eliminating transitive compilation of unused code.

---

## Risks and mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| `pub(crate)` items in engine accessed by knowledge_store | Build failure | Audit all `pub(crate)` items consumed cross-module; promote to `pub` or add re-exports |
| Circular deps between graph_intelligence and knowledge_store | Can't compile | Move `impl KnowledgeStore` graph methods into knowledge_store module, use functions for pure scoring |
| Feature flag complexity increases | Confusing builds | Document feature matrix in workspace README; add CI jobs per feature combination |
| Test files reference multiple modules | Test compilation breaks | Integration tests stay in `aletheia-mneme` facade or move to `integration-tests` crate |
| error.rs has variants from all domains | Can't split cleanly | Split error enum per crate; facade re-exports a unified enum or uses `#[snafu(module)]` |

---

## Open questions

1. **Facade vs direct deps:** Should downstream crates keep depending on the
   facade, or migrate to direct sub-crate deps? Direct deps give finer
   incremental builds but increase Cargo.toml maintenance.

2. **Error enum split:** `Error` currently has 21 variants spanning session,
   knowledge, engine, and I/O concerns. Split into per-crate errors with
   `From` impls, or keep a unified enum in the facade?

3. **Engine vendoring strategy:** The engine is a vendored fork of CozoDB. If
   it were published as a separate workspace crate, could it be shared with
   other projects or versioned independently?

4. **Embedding provider placement:** The `EmbeddingProvider` trait is used by
   both knowledge_store and nous. Placing it in `mneme-types` (as proposed)
   keeps it leaf-level but adds a trait to what is otherwise a pure data crate.

---

## Observations (out of scope)

- **Debt:** `engine/fts/tokenizer/stop_word_filter/stopwords.rs` is ~22K
  lines of static word lists. Consider loading from a compressed asset at
  runtime or build-time `include_bytes!` to reduce source churn.
  (`crates/mneme/src/engine/fts/tokenizer/stop_word_filter/stopwords.rs`)

- **Debt:** Engine modules use blanket `#[expect(clippy::pedantic)]` at module
  level. After extraction to own crate, these can be narrowed to specific
  lints per function. (`crates/mneme/src/engine/mod.rs:30-97`)

- **Idea:** `mneme-session` could become a generic embedded session store
  usable outside aletheia, since it has no domain-specific logic beyond types.

- **Missing test:** No integration test verifies that the facade re-exports
  match the individual crate APIs. Add a compile-test after the split.

- **Doc gap:** No documentation on which feature flag combinations are valid
  or tested in CI. A feature matrix table would prevent broken combinations.
