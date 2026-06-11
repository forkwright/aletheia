# L3 API Index: aletheia-sessions-migrate

Crate path: `crates/aletheia-sessions-migrate`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/commands/report.rs`

> Print the dry-run plan to stdout in operator-friendly form.
```rust
pub fn print_dry_run (plan: &MigrationPlan, dest: &Path)
```

> Print the migration report to stdout.
```rust
pub fn print_migration (r: &MigrationReport)
```

> Print the verification report to stdout.
```rust
pub fn print_verification (v: &VerificationReport)
```

## `src/dest.rs`

> Partitions the runtime expects (mirrors `fjall_store::PARTITIONS`)
> plus the migrator's `migration_legacy` sidecar.
```rust
pub const ALL_PARTITIONS: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "distillations",
    "notes",
    "blackboard",
    "counters",
    "migration_legacy",
];
```

> Helper that owns the fjall handle and named partitions during migration.
```rust
pub struct Destination {
    db: FjallDb,
    /// `sessions` partition handle.
    pub sessions: SingleWriterTxKeyspace,
    /// `messages` partition handle.
    pub messages: SingleWriterTxKeyspace,
    /// `usage` partition handle.
    pub usage: SingleWriterTxKeyspace,
    /// `distillations` partition handle.
    pub distillations: SingleWriterTxKeyspace,
    /// `notes` partition handle.
    pub notes: SingleWriterTxKeyspace,
    /// `blackboard` partition handle.
    pub blackboard: SingleWriterTxKeyspace,
    /// `counters` partition handle.
    pub counters: SingleWriterTxKeyspace,
    /// `migration_legacy` partition handle.
    pub migration_legacy: SingleWriterTxKeyspace,
}
```

```rust
impl Destination {
    pub fn open (path: &Path, force: bool) -> Result<Self>;
    pub fn write_all (
        &self,
        sessions: &[(Session, LegacyExtras)],
        messages: &[Message],
        usage: &[UsageRecord],
        distillations: &[DistillationRecord],
        notes: &[AgentNote],
        blackboard: &[BlackboardRow],
    ) -> Result<TableCounts>;
    pub fn count_sessions (&self) -> Result<usize>;
}
```

```rust
pub struct TableCounts {
    /// Sessions written (including any synthesised orphan-recovery sessions).
    pub sessions: usize,
    /// Messages written.
    pub messages: usize,
    /// Usage records written.
    pub usage: usize,
    /// Distillations written.
    pub distillations: usize,
    /// Agent notes written.
    pub notes: usize,
    /// Blackboard entries written.
    pub blackboard: usize,
}
```

## `src/error.rs`

> Result alias for migrator operations.
```rust
pub type Result<T, E = Error> = std::result::Result<T, E>;
```

```rust
pub enum Error {
    #[snafu(display("opening SQLite source at {}: {source}", path.display()))]
    SqliteOpen {
        path: PathBuf,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("SQLite query error ({context}): {source}"))]
    Sqlite {
        context: String,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "schema mismatch: expected user_version = {expected}, found {found}; \
         this migrator only supports the final pre-fjall SQLite schema (PR #3446)"
    ))]
    SchemaUserVersion {
        expected: i64,
        found: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("schema mismatch: required table '{table}' not present in source DB"))]
    SchemaMissingTable {
        table: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "schema mismatch: table '{table}' missing column '{column}' (found columns: {found:?})"
    ))]
    SchemaMissingColumn {
        table: String,
        column: String,
        found: Vec<String>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("opening fjall destination at {}: {message}", path.display()))]
    FjallOpen {
        path: PathBuf,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "destination '{}' is non-empty; pass --force to migrate over an existing fjall directory \
         (data already there will NOT be removed)",
        path.display()
    ))]
    DestinationNotEmpty {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fjall partition '{partition}': {message}"))]
    FjallPartition {
        partition: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fjall {operation} failed: {message}"))]
    FjallOp {
        operation: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("JSON {operation}: {source}"))]
    Json {
        operation: String,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("graphe SessionStore error: {source}"))]
    Graphe {
        source: graphe::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("io error ({context}): {source}"))]
    Io {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display(
        "{field} value {value} cannot be encoded as a u64 (must be non-negative and fit in 64 bits)"
    ))]
    NumericRange {
        field: String,
        value: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/migrate.rs`

> Field-mapping documentation, exported so tests can sanity-check it
> stays in sync with the actual mapper.
```rust
pub const FIELD_MAPPING_DOC: &str = include_str!("../FIELD_MAPPING.md");
```

```rust
pub struct MigrationPlan {
    /// Path to the `SQLite` source DB.
    pub source: PathBuf,
    /// Path the migrator would write fjall data to.
    pub dest: PathBuf,
    /// Per-table counts read from the source (excludes synthesised orphans).
    pub counts: TableCounts,
    /// First `session_id` the migrator would touch, for log line cross-reference.
    pub sample_session_id: Option<String>,
    /// Sessions that carry non-default `thinking_*` / `working_state` /
    /// `distillation_priming` columns; preserved in `migration_legacy`.
    pub legacy_extras_present: usize,
    /// Orphan messages (session row missing) that the migrator would
    /// preserve under synthesised orphan-recovery sessions.
    pub orphan_messages_detected: usize,
    /// Number of distinct orphan `session_ids` the migrator would synthesise.
    pub orphan_sessions_to_synthesise: usize,
}
```

```rust
pub struct MigrationReport {
    /// Path to the `SQLite` source DB.
    pub source: PathBuf,
    /// Path the migrator wrote fjall data to.
    pub dest: PathBuf,
    /// Per-table counts written (sessions includes synthesised orphans).
    pub counts: TableCounts,
    /// Sessions whose legacy extras were preserved in `migration_legacy`.
    pub legacy_extras_preserved: usize,
    /// Count of orphan messages (whose parent session row was missing in
    /// the legacy DB) that were preserved under synthesised
    /// `orphan-recovery` sessions.
    pub orphan_messages_recovered: usize,
    /// Number of synthesised orphan-recovery sessions.
    pub orphan_sessions_synthesised: usize,
    /// Wall time of the migration.
    pub elapsed_secs: f64,
}
```

> Open the source `SQLite` DB read-only with a sane busy-timeout.
> 
> # Errors
> 
> Returns [`crate::error::Error::SqliteOpen`] when the source path
> cannot be opened or the busy-timeout PRAGMA fails to apply.
```rust
pub fn open_source (path: &Path) -> Result<Connection>
```

> Run a dry-run plan: read source, validate, summarise. No fjall writes.
> 
> # Errors
> 
> Propagates schema validation failures and any source read errors.
```rust
pub fn run_dry_run (source: &Path) -> Result<MigrationPlan>
```

> Run a full migration. Reads source, validates, writes fjall.
> 
> # Errors
> 
> Propagates schema validation failures, source read errors, and any
> fjall write failure as the structured error type defined in
> [`crate::error`].
```rust
pub fn run_migration (source: &Path, dest: &Path, force: bool) -> Result<MigrationReport>
```

## `src/schema.rs`

> Schema version we know how to migrate.
> 
> Anchored on the final `SQLite` revision shipped before PR #3446 removed
> the `SQLite` backend. The operator's real DB shows `PRAGMA user_version
> = 32`.
```rust
pub const REQUIRED_USER_VERSION: i64 = 32;
```

> Tables we expect to read from. Other tables (`planning_*`, `audit_log`,
> `thread_summaries`, etc.) exist in v32 but are not part of the
> `SessionStore` surface  -  the new fjall layout has no analog for them
> and they were never accessed via the public `SessionStore` API.
```rust
pub const REQUIRED_TABLES: &[&str] = &[
    "sessions",
    "messages",
    "usage",
    "distillations",
    "agent_notes",
    "blackboard",
];
```

```rust
pub fn required_columns (table: &str) -> &'static [&'static str]
```

> Validate that the connection points at a v32 `SQLite` session DB with
> every required table and column.
> 
> On mismatch, returns a specific error naming the missing item.
> 
> # Errors
> 
> Returns [`crate::error::Error::SchemaUserVersion`] if `PRAGMA user_version`
> is not [`REQUIRED_USER_VERSION`], [`crate::error::Error::SchemaMissingTable`]
> or [`crate::error::Error::SchemaMissingColumn`] when the source schema
> drifts from what the migrator can read.
```rust
pub fn validate (conn: &Connection) -> Result<()>
```

## `src/source.rs`

```rust
pub struct LegacyExtras {
    /// `thinking_enabled` flag (0/1). Default: 0.
    pub thinking_enabled: Option<i64>,
    /// `thinking_budget` token cap. Default: 10000.
    pub thinking_budget: Option<i64>,
    /// `working_state` opaque blob (TEXT JSON). Default: NULL.
    pub working_state: Option<String>,
    /// `distillation_priming` opaque blob (TEXT). Default: NULL.
    pub distillation_priming: Option<String>,
}
```

```rust
impl LegacyExtras {
    pub fn is_non_default (&self) -> bool;
}
```

```rust
pub struct SessionRow {
    /// Session record in the new fjall shape.
    pub session: Session,
    /// Legacy columns that don't map to the new shape.
    pub legacy: LegacyExtras,
}
```

> Read every session.
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] if the SELECT cannot be
> prepared, executed, or any row fails to map.
```rust
pub fn read_sessions (conn: &Connection) -> Result<Vec<SessionRow>>
```

> Read every message, ordered by (`session_id`, `seq`).
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
```rust
pub fn read_messages (conn: &Connection) -> Result<Vec<Message>>
```

> Read every usage record.
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
```rust
pub fn read_usage (conn: &Connection) -> Result<Vec<UsageRecord>>
```

```rust
pub struct DistillationRecord {
    /// Owning session.
    pub session_id: String, // kanon:ignore RUST/primitive-for-domain-id WHY: mirrors legacy SQLite schema byte-for-byte; newtype would break serde deserialization
    /// Message count before distillation.
    pub messages_before: i64,
    /// Message count after distillation.
    pub messages_after: i64,
    /// Token count before distillation.
    pub tokens_before: i64,
    /// Token count after distillation.
    pub tokens_after: i64,
    /// Model that produced the summary.
    pub model: Option<String>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}
```

> Read every distillation record, ordered by `(session_id, id)` so the
> per-session local sequence we assign matches insertion order.
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
```rust
pub fn read_distillations (conn: &Connection) -> Result<Vec<DistillationRecord>>
```

> Read every agent note, ordered by `(session_id, id)` so the per-session
> local sequence we assign matches insertion order.
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
```rust
pub fn read_notes (conn: &Connection) -> Result<Vec<AgentNote>>
```

> Read every blackboard entry. We keep expired entries here too  -  the
> fjall layer will filter them at read time, and operators may want
> to inspect them post-migration.
> 
> # Errors
> 
> Returns [`crate::error::Error::Sqlite`] on prepare / query / map failure.
```rust
pub fn read_blackboard (conn: &Connection) -> Result<Vec<BlackboardRow>>
```

## `src/verify.rs`

```rust
pub struct VerificationReport {
    /// Number of per-session samples spot-checked.
    pub samples_checked: usize,
    /// Each detected mismatch as a human-readable line.
    pub mismatches: Vec<String>,
    /// Sessions known to the source (real rows + distinct orphan IDs).
    pub source_session_count: usize,
    /// Sessions present in dest fjall.
    pub dest_session_count: usize,
    /// Messages in source.
    pub source_message_count: usize,
    /// Messages in dest fjall.
    pub dest_message_count: usize,
    /// Whether the SHA-256 of every message body matches between stores.
    pub message_body_hash_match: bool,
    /// Source-side hash, hex-encoded.
    pub source_message_body_sha256: String,
    /// Destination-side hash, hex-encoded.
    pub dest_message_body_sha256: String,
}
```

```rust
impl VerificationReport {
    pub fn ok (&self) -> bool;
}
```

> Run a verification pass against a freshly-migrated fjall directory.
> 
> # Errors
> 
> Propagates `SQLite` and fjall scan errors.
```rust
pub fn run_verification (source: &Path, dest: &Path, samples: usize) -> Result<VerificationReport>
```

## `tests/common/mod.rs`

> Minimal v32 schema covering every column the migrator reads.
> 
> This is intentionally hand-written rather than imported from the
> historical migration runner  -  the migrator only knows about column
> presence + types, so the simplest fixture is the one that mirrors
> the live shape one-to-one.
```rust
pub const SCHEMA_SQL: &str = "
PRAGMA user_version = 32;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    nous_id TEXT NOT NULL,
    session_key TEXT NOT NULL,
    parent_session_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    model TEXT,
    token_count_estimate INTEGER DEFAULT 0,
    message_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_input_tokens INTEGER DEFAULT 0,
    bootstrap_hash TEXT,
    distillation_count INTEGER DEFAULT 0,
    thinking_enabled INTEGER DEFAULT 0,
    thinking_budget INTEGER DEFAULT 10000,
    thread_id TEXT,
    transport TEXT,
    working_state TEXT,
    session_type TEXT DEFAULT 'primary',
    last_distilled_at TEXT,
    computed_context_tokens INTEGER DEFAULT 0,
    distillation_priming TEXT,
    display_name TEXT,
    UNIQUE(nous_id, session_key)
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    token_estimate INTEGER DEFAULT 0,
    is_distilled INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(session_id, seq)
);

CREATE TABLE usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    turn_seq INTEGER NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    model TEXT,
    created_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE distillations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    messages_before INTEGER NOT NULL,
    messages_after INTEGER NOT NULL,
    tokens_before INTEGER NOT NULL,
    tokens_after INTEGER NOT NULL,
    facts_extracted INTEGER DEFAULT 0,
    model TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE agent_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    nous_id TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'context',
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE blackboard (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    author_nous_id TEXT NOT NULL,
    ttl_seconds INTEGER DEFAULT 3600,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
";
```

```rust
pub fn build_empty_v32 (path: &Path)
```

```rust
pub fn insert_session (
    conn: &Connection,
    id: &str,
    nous_id: &str,
    session_key: &str,
    status: &str,
    model: Option<&str>,
    created_at: &str,
    updated_at: &str,
)
```

```rust
pub fn insert_message (
    conn: &Connection,
    session_id: &str,
    seq: i64,
    role: &str,
    content: &str,
    is_distilled: bool,
    token_estimate: i64,
    created_at: &str,
)
```

```rust
pub fn insert_distillation (
    conn: &Connection,
    session_id: &str,
    messages_before: i64,
    messages_after: i64,
    tokens_before: i64,
    tokens_after: i64,
    model: Option<&str>,
    created_at: &str,
)
```

```rust
pub fn insert_note (
    conn: &Connection,
    session_id: &str,
    nous_id: &str,
    category: &str,
    content: &str,
    created_at: &str,
)
```

```rust
pub fn insert_usage (
    conn: &Connection,
    session_id: &str,
    turn_seq: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read: i64,
    cache_write: i64,
    model: Option<&str>,
)
```
