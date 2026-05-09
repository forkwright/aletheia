# L3 API Index: krites

Crate path: `crates/krites`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/async_surface.rs`

```rust
pub struct AsyncDb {
    inner: Arc<Db>,
}
```

```rust
impl AsyncDb {
    pub async fn open_mem () -> crate::Result<Self>;
    pub async fn open_fjall (path: impl AsRef<Path> + Send + 'static) -> crate::Result<Self>;
    pub fn with_cache (self, capacity: NonZeroUsize) -> Self;
    pub async fn cache_stats (&self) -> Option<QueryCacheStats>;
    pub async fn run (
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> crate::Result<NamedRows>;
    pub async fn run_read_only (
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::Result<NamedRows>;
    pub async fn backup_db (
        &self,
        out_file: impl AsRef<Path> + Send + 'static,
    ) -> crate::Result<()>;
    pub async fn restore_backup (
        &self,
        in_file: impl AsRef<Path> + Send + 'static,
    ) -> crate::Result<()>;
    pub async fn import_from_backup (
        &self,
        in_file: impl AsRef<Path> + Send + 'static,
        relations: &[String],
    ) -> crate::Result<()>;
    pub async fn export_relations <I, T> (
        &self,
        relations: I,
    ) -> crate::Result<BTreeMap<String, NamedRows>> where
        I: Iterator<Item = T> + Send,
        T: AsRef<str> + Send,;
    pub async fn import_relations (&self, data: BTreeMap<String, NamedRows>) -> crate::Result<()>;
    pub async fn register_fixed_rule <R: FixedRule + 'static> (
        &self,
        name: String,
        rule: R,
    ) -> crate::Result<()>;
    pub async fn register_callback (
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (
        u32,
        crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>,
    );
    pub async fn multi_transaction (&self, write: bool) -> crate::MultiTransaction;
}
```

## `src/counterfactual.rs`

```rust
pub struct CausalEdgeRow {
    /// Cause fact ID.
    pub cause: String,
    /// Effect fact ID.
    pub effect: String,
    /// Relationship type (caused, enabled, prevented, correlated).
    pub relationship_type: CausalRelationType,
    /// Edge confidence in `[0.0, 1.0]`.
    pub confidence: f64,
}
```

> Typed query builders for counterfactual reasoning over causal graphs.
```rust
pub struct Counterfactual;
```

```rust
impl Counterfactual {
    pub fn dependency_analysis (db: &Db, fact_id: impl AsRef<str>) -> Result<Vec<CausalEdgeRow>>;
    pub fn impact_analysis (db: &Db, fact_id: impl AsRef<str>) -> Result<Vec<CausalEdgeRow>>;
    pub fn minimal_provenance (
        db: &Db,
        conclusion_id: impl AsRef<str>,
    ) -> Result<Vec<CausalEdgeRow>>;
}
```

## `src/data/expr/expr_impl.rs`

```rust
pub enum Expr {
    /// Binding to variables
    Binding {
        /// The variable name to bind
        var: Symbol,
        /// When executing in the context of a tuple, the position of the binding within the tuple
        tuple_pos: Option<usize>,
    },
    /// Constant expression containing a value
    Const {
        /// The value
        val: DataValue,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Function application
    Apply {
        /// Op representing the function to apply
        op: &'static Op,
        /// Arguments to the application
        args: Box<[Expr]>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Unbound function application
    UnboundApply {
        /// Op representing the function to apply
        op: CompactString,
        /// Arguments to the application
        args: Box<[Expr]>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
    /// Conditional expressions
    Cond {
        /// Conditional clauses, the first expression in each tuple should evaluate to a boolean
        clauses: Vec<(Expr, Expr)>,
        /// Source span
        #[serde(skip)]
        span: SourceSpan,
    },
}
```

```rust
impl Expr {
    pub fn eval_to_const (mut self) -> Result<DataValue>;
}
```

## `src/data/expr/mod.rs`

```rust
pub enum Bytecode {
    /// push 1
    Binding {
        var: Symbol,
        tuple_pos: Option<usize>,
    },
    /// push 1
    Const {
        val: DataValue,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop n, push 1
    Apply {
        op: &'static Op,
        arity: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// pop 1
    JumpIfFalse {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
    /// unchanged
    Goto {
        jump_to: usize,
        #[serde(skip)]
        span: SourceSpan,
    },
}
```

> Evaluate bytecode to a boolean predicate result.
>
> # Errors
>
> Returns an error if bytecode evaluation fails or if the result
> is not a boolean value.
```rust
pub fn eval_bytecode_pred (
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
    _span: SourceSpan,
) -> Result<bool>
```

> Evaluate bytecode to produce a data value.
>
> # Errors
>
> Returns an error if a variable is unbound, if the tuple is too short
> for a binding, if an operation fails, or if type mismatches occur.
```rust
pub fn eval_bytecode (
    bytecodes: &[Bytecode],
    bindings: impl AsRef<[DataValue]>,
    stack: &mut Vec<DataValue>,
) -> Result<DataValue>
```

## `src/data/expr/op.rs`

```rust
pub struct Op {
    pub(crate) name: &'static str,
    pub(crate) min_arity: usize,
    pub(crate) vararg: bool,
    pub(crate) inner: fn(&[DataValue]) -> DataResult<DataValue>,
}
```

```rust
pub trait CustomOp {
    fn name (&self) -> &'static str;
    fn min_arity (&self) -> usize;
    fn vararg (&self) -> bool;
    fn return_type (&self) -> NullableColType;
    fn call (&self, args: &[DataValue]) -> Result<DataValue>;
}
```

## `src/data/memcmp.rs`

```rust
pub fn decode_bytes (data: &[u8]) -> (Vec<u8>, &[u8])
```

## `src/data/relation.rs`

```rust
pub struct NullableColType {
    pub coltype: ColType,
    pub nullable: bool,
}
```

```rust
pub enum ColType {
    Any,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Uuid,
    List {
        eltype: Box<NullableColType>,
        len: Option<usize>,
    },
    Vec {
        eltype: VecElementType,
        len: usize,
    },
    Tuple(Vec<NullableColType>),
    Validity,
    Json,
}
```

```rust
pub enum VecElementType {
    F32,
    F64,
}
```

## `src/data/symb.rs`

```rust
pub struct Symbol {
    pub(crate) name: CompactString,
    #[serde(skip)]
    pub(crate) span: SourceSpan,
}
```

## `src/data/tuple.rs`

```rust
pub type Tuple = Vec<DataValue>;
```

```rust
pub fn decode_tuple_from_key (key: &[u8], size_hint: usize) -> Tuple
```

> Check if the tuple key passed in should be a valid return for a validity query.
>
> Returns two elements, the first element contains `Some(tuple)` if the key should be included
> in the return set and `None` otherwise,
> the second element gives the next binary key for the seek to be used as an inclusive
> lower bound.
```rust
pub fn check_key_for_validity (
    key: &[u8],
    valid_at: ValidityTs,
    size_hint: Option<usize>,
) -> (Option<Tuple>, Vec<u8>)
```

## `src/data/value.rs`

```rust
pub struct UuidWrapper(pub Uuid);
```

```rust
pub struct RegexWrapper(pub Regex);
```

```rust
pub struct ValidityTs(pub Reverse<i64>);
```

```rust
pub struct Validity {
    /// Microsecond timestamp, sorted descending (newest first).
    pub timestamp: ValidityTs,
    /// `true` = assertion, `false` = retraction; sorted descending.
    pub is_assert: Reverse<bool>,
}
```

```rust
pub enum DataValue {
    /// The null (absent) value. Sorts before all other variants.
    Null,
    /// Boolean truth value.
    Bool(bool),
    /// Numeric value — integer or float (see [`Num`]).
    Num(Num),
    /// UTF-8 string, stored inline via [`CompactString`].
    Str(CompactString),
    /// Raw byte sequence, serialized via `serde_bytes`.
    #[serde(with = "serde_bytes")]
    Bytes(Vec<u8>),
    /// UUID value with chronological sort order (see [`UuidWrapper`]).
    Uuid(UuidWrapper),
    /// Compiled regex — transient, not serializable. Engine-internal.
    Regex(RegexWrapper),
    /// Ordered sequence of values.
    List(Vec<DataValue>),
    /// Deduplicated ordered set. Engine-internal; coerced to `List` at output.
    Set(BTreeSet<DataValue>),
    /// Typed floating-point vector for proximity search (HNSW).
    Vec(Vector),
    /// Arbitrary JSON value (objects, arrays, etc.).
    Json(JsonData),
    /// Timestamp + assertion flag for time-travel queries. Engine-internal.
    Validity(Validity),
    /// Bottom sentinel — sorts after everything. Used as upper key bound. Engine-internal.
    Bot,
}
```

```rust
pub struct JsonData(pub JsonValue);
```

```rust
pub enum Vector {
    /// Single-precision (32-bit) float array.
    F32(Array1<f32>),
    /// Double-precision (64-bit) float array.
    F64(Array1<f64>),
}
```

```rust
impl Vector {
    pub fn len (&self) -> usize;
    pub fn is_empty (&self) -> bool;
}
```

```rust
pub enum Num {
    /// Exact integer value.
    Int(i64),
    /// IEEE 754 double-precision floating-point value.
    Float(f64),
}
```

```rust
impl Num {
    pub fn get_int (&self) -> Option<i64>;
}
```

```rust
impl DataValue {
    pub fn get_bytes (&self) -> Option<&[u8]>;
    pub fn get_slice (&self) -> Option<&[DataValue]>;
    pub fn get_str (&self) -> Option<&str>;
    pub fn get_int (&self) -> Option<i64>;
    pub fn get_float (&self) -> Option<f64>;
    pub fn get_bool (&self) -> Option<bool>;
}
```

## `src/error.rs`

```rust
pub enum Error {
    /// A database engine operation failed.
    #[snafu(display("{message}"))]
    Engine {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A running query was cancelled via poison/timeout.
    #[snafu(display("Running query was killed before completion"))]
    QueryKilled {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A parse error (query syntax).
    #[snafu(display("parse error: {source}"))]
    Parse {
        source: crate::parse::error::ParseError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A storage error.
    #[snafu(display("storage error: {source}"))]
    Storage {
        source: crate::storage::error::StorageError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Result alias using the engine's public [`Error`] type.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## `src/fixed_rule/csr/mod.rs`

```rust
impl <EV> Target<EV> {
    pub fn new (target: u32, value: EV) -> Self;
}
```

```rust
impl <EV> DirectedCsrGraph<EV> {
    pub fn node_count (&self) -> u32;
    pub fn out_degree (&self, node: u32) -> u32;
    pub fn out_neighbors_with_values (&self, node: u32) -> std::slice::Iter<'_, Target<EV>>;
}
```

```rust
impl DirectedCsrGraph<()> {
    pub fn out_neighbors (&self, node: u32) -> impl Iterator<Item = u32> + '_;
    pub fn in_neighbors (&self, node: u32) -> impl Iterator<Item = u32> + '_;
}
```

```rust
impl <EV: Copy> CsrBuilder<EV> {
    pub fn new () -> Self;
    pub fn sorted (mut self) -> Self;
    pub fn edges_with_values (mut self, edges: impl IntoIterator<Item = (u32, u32, EV)>) -> Self;
    pub fn build (self) -> DirectedCsrGraph<EV>;
}
```

```rust
impl CsrBuilder<()> {
    pub fn edges (mut self, edges: impl IntoIterator<Item = (u32, u32)>) -> Self;
}
```

## `src/fixed_rule/csr/page_rank.rs`

```rust
impl PageRankConfig {
    pub fn new (max_iterations: usize, tolerance: f64, damping_factor: f32) -> Self;
}
```

## `src/fixed_rule/mod.rs`

> Passed into implementation of fixed rule, can be used to obtain relation inputs and options
```rust
pub struct FixedRulePayload<'a, 'b> {
    pub(crate) manifest: &'a MagicFixedRuleApply,
    pub(crate) stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    pub(crate) tx: &'a SessionTx<'b>,
}
```

```rust
pub struct FixedRuleInputRelation<'a, 'b> {
    arg_manifest: &'a MagicFixedRuleRuleArg,
    stores: &'a BTreeMap<MagicSymbol, EpochStore>,
    tx: &'a SessionTx<'b>,
}
```

```rust
impl <'a, 'b> FixedRuleInputRelation<'a, 'b> {
    pub fn arity (&self) -> Result<usize>;
    pub fn ensure_min_len (self, len: usize) -> Result<Self>;
    pub fn get_binding_map (&self, offset: usize) -> BTreeMap<Symbol, usize>;
    pub fn iter (&self) -> Result<TupleIter<'a>>;
    pub fn prefix_iter (&self, prefix: &DataValue) -> Result<TupleIter<'_>>;
    pub fn span (&self) -> SourceSpan;
    pub fn as_directed_graph (
        &self,
        undirected: bool,
    ) -> Result<(DirectedCsrGraph, Vec<DataValue>, BTreeMap<DataValue, u32>)>;
    pub fn as_directed_weighted_graph (
        &self,
        undirected: bool,
        allow_negative_weights: bool,
    ) -> Result<(
        DirectedCsrGraph<f32>,
        Vec<DataValue>,
        BTreeMap<DataValue, u32>,
    )>;
}
```

```rust
impl <'a, 'b> FixedRulePayload<'a, 'b> {
    pub fn inputs_count (&self) -> usize;
    pub fn get_input (&self, idx: usize) -> Result<FixedRuleInputRelation<'a, 'b>>;
    pub fn name (&self) -> &str;
    pub fn span (&self) -> SourceSpan;
    pub fn expr_option (&self, name: &str, default: Option<Expr>) -> Result<Expr>;
    pub fn string_option (&self, name: &str, default: Option<&str>) -> Result<CompactString>;
    pub fn option_span (&self, name: &str) -> Result<SourceSpan>;
    pub fn integer_option (&self, name: &str, default: Option<i64>) -> Result<i64>;
    pub fn pos_integer_option (&self, name: &str, default: Option<usize>) -> Result<usize>;
    pub fn non_neg_integer_option (&self, name: &str, default: Option<usize>) -> Result<usize>;
    pub fn float_option (&self, name: &str, default: Option<f64>) -> Result<f64>;
    pub fn unit_interval_option (&self, name: &str, default: Option<f64>) -> Result<f64>;
    pub fn bool_option (&self, name: &str, default: Option<bool>) -> Result<bool>;
}
```

```rust
pub trait FixedRule : Send + Sync {
    fn init_options (
        &self,
        _options: &mut BTreeMap<CompactString, Expr>,
        _span: SourceSpan,
    ) -> Result<()>; // default impl
    fn arity (
        &self,
        options: &BTreeMap<CompactString, Expr>,
        rule_head: &[Symbol],
        span: SourceSpan,
    ) -> Result<usize>;
    fn run (
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &'_ mut RegularTempStore,
        poison: Poison,
    ) -> Result<()>;
}
```

## `src/fts/mod.rs`

```rust
pub struct TokenizerConfig {
    pub name: CompactString,
    pub args: Vec<DataValue>,
}
```

## `src/hot_reload.rs`

> File extension for Datalog rule files loaded by the hot-reloader.
```rust
pub const RULE_EXTENSION: &str = "mnm";
```

```rust
pub enum ReloadEvent {
    /// Rules were successfully reloaded.
    Reloaded {
        /// Number of source files loaded.
        count: usize,
    },
    /// A parse error prevented reload; old ruleset retained.
    ParseError {
        /// Human-readable error message.
        source: String,
    },
}
```

```rust
pub struct RuleSource {
    /// Filename (not full path) of the source file.
    pub filename: String,
    /// UTC timestamp of last successful load.
    pub last_loaded: jiff::Timestamp,
}
```

```rust
pub struct RuleSet {
    /// Concatenated Datalog rule text from all source files.
    pub rules_text: Arc<str>,
    /// Per-source metadata for health/observability.
    pub sources: Vec<RuleSource>,
    /// Number of source files.
    pub source_count: usize,
}
```

```rust
pub enum HotReloadError {
    /// Failed to initialize the file watcher.
    #[snafu(display("failed to initialize file watcher"))]
    WatcherInit {
        /// Underlying notify error.
        source: notify::Error,
    },
    /// Failed to read the rule directory.
    #[snafu(display("failed to read rule directory {path}"))]
    ReadDir {
        /// Directory path.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to read a rule file.
    #[snafu(display("failed to read rule file {path}"))]
    ReadFile {
        /// File path.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Rule text failed to parse.
    #[snafu(display("rule parse error: {message}"))]
    Parse {
        /// Parse error message.
        message: String,
    },
}
```

```rust
pub struct HotReloader {
    rule_dir: PathBuf,
    reload_tx: mpsc::Sender<ReloadEvent>,
    _watcher: notify::RecommendedWatcher,
}
```

```rust
impl HotReloader {
    pub fn start (
        rule_dir: impl AsRef<Path>,
        fixed_rules: &Arc<
            crossbeam::sync::ShardedLock<BTreeMap<String, Arc<Box<dyn crate::FixedRule>>>>,
        >,
    ) -> Result<(Self, mpsc::Receiver<ReloadEvent>, Arc<ArcSwap<RuleSet>>), HotReloadError>;
}
```

## `src/lib.rs`

> Public facade for the Datalog engine. Dispatches to a concrete storage backend.
>
> Obtain an instance via [`Db::open_mem`] or [`Db::open_fjall`]. Attach an
> optional LRU query cache with [`Db::with_cache`] to track hit/miss metrics
> for repeated Datalog queries.
```rust
pub struct Db {
    inner: DbInner,
    /// Optional LRU cache that records whether each normalized query string has
    /// been seen before, exposing hit/miss metrics for observability.
    cache: Option<Arc<QueryCache>>,
}
```

```rust
impl Db {
    pub fn open_mem () -> crate::Result<Self>;
    pub fn open_fjall (path: impl AsRef<Path>) -> crate::Result<Self>;
    pub fn with_cache (mut self, capacity: NonZeroUsize) -> Self;
    pub fn with_rule_store (
        mut self,
        store: Arc<arc_swap::ArcSwap<crate::hot_reload::RuleSet>>,
    ) -> Self;
    pub fn cache_stats (&self) -> Option<QueryCacheStats>;
    pub fn run (
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> crate::Result<NamedRows>;
    pub fn run_read_only (
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
    ) -> crate::Result<NamedRows>;
    pub fn backup_db (&self, out_file: impl AsRef<Path>) -> crate::Result<()>;
    pub fn restore_backup (&self, in_file: impl AsRef<Path>) -> crate::Result<()>;
    pub fn import_from_backup (
        &self,
        in_file: impl AsRef<Path>,
        relations: &[String],
    ) -> crate::Result<()>;
    pub fn export_relations <I, T> (&self, relations: I) -> crate::Result<BTreeMap<String, NamedRows>> where
        I: Iterator<Item = T>,
        T: AsRef<str>,;
    pub fn import_relations (&self, data: BTreeMap<String, NamedRows>) -> crate::Result<()>;
    pub fn register_fixed_rule <R: FixedRule + 'static> (
        &self,
        name: String,
        rule: R,
    ) -> crate::Result<()>;
    pub fn register_callback (
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (
        u32,
        crossbeam::channel::Receiver<(CallbackOp, NamedRows, NamedRows)>,
    );
    pub fn multi_transaction (&self, write: bool) -> MultiTransaction;
}
```

```rust
pub struct MultiTransaction {
    /// Commands can be sent into the transaction through this channel
    pub sender: Sender<TransactionPayload>,
    /// Results can be retrieved from the transaction from this channel
    pub receiver: Receiver<crate::error::InternalResult<NamedRows>>,
}
```

## `src/parse/error.rs`

```rust
pub enum ParseError {
    /// Pest-level syntax error: unexpected token or end-of-input.
    #[snafu(display("syntax error at {span}: {message}"))]
    Syntax {
        span: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Semantically invalid query structure caught after parsing.
    #[snafu(display("invalid query: {message}"))]
    InvalidQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Semantically invalid query structure with source location.
    #[snafu(display("invalid query at {span}: {message}"))]
    InvalidQueryAt {
        span: SourceSpan,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Reference to a variable that is not bound in any rule head.
    #[snafu(display("unbound variable '{name}'"))]
    UnboundVariable {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A grammar rule appeared where it was not expected.
    ///
    /// This indicates a mismatch between the pest grammar and the parser
    /// implementation. If encountered, file a bug report.
    #[snafu(display("unexpected grammar rule {rule:?} at {span} in {context}"))]
    UnexpectedRule {
        rule: String,
        span: SourceSpan,
        context: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A required child element was missing from a pest parse pair.
    #[snafu(display("missing {element} at {span}"))]
    MissingElement {
        element: &'static str,
        span: SourceSpan,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Integer literal could not be parsed.
    #[snafu(display("invalid integer literal at {span}: {message}"))]
    InvalidInteger {
        span: SourceSpan,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Raw pest parser failure: wraps the pest error so span details are preserved.
    ///
    /// Constructed by callers that have a pest error in hand; `parse_script` uses
    /// [`Syntax`] instead so it can attach span information inline.
    #[snafu(display("parse failed: {source}"))]
    PestError {
        source: pest::error::Error<super::Rule>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/parse/mod.rs`

```rust
pub enum DatalogScript {
    /// A single query program.
    Single(InputProgram),
    /// An imperative script with control flow.
    Imperative(ImperativeProgram),
    /// A system command (`:compact`, `:explain`, etc.).
    Sys(SysOp),
}
```

```rust
pub struct ImperativeStmtClause {
    /// The parsed query program.
    pub prog: InputProgram,
    /// Optional name to store results into a temporary relation.
    pub store_as: Option<CompactString>,
}
```

```rust
pub struct ImperativeSysop {
    /// The parsed system operation.
    pub sysop: SysOp,
    /// Optional name to store results into a temporary relation.
    pub store_as: Option<CompactString>,
}
```

```rust
pub enum ImperativeStmt {
    /// Exit the nearest (or named) enclosing loop.
    Break {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    /// Skip to the next iteration of the nearest (or named) enclosing loop.
    Continue {
        target: Option<CompactString>,
        span: SourceSpan,
    },
    /// Return results from an imperative block.
    Return {
        returns: Vec<Either<ImperativeStmtClause, CompactString>>,
    },
    /// Execute a query program.
    Program { prog: ImperativeStmtClause },
    /// Execute a system operation.
    SysOp { sysop: ImperativeSysop },
    /// Execute a query, suppressing any errors.
    IgnoreErrorProgram { prog: ImperativeStmtClause },
    /// Conditional branch.
    If {
        condition: ImperativeCondition,
        then_branch: ImperativeProgram,
        else_branch: ImperativeProgram,
        negated: bool,
    },
    /// Infinite loop with optional label.
    Loop {
        label: Option<CompactString>,
        body: ImperativeProgram,
    },
    /// Swap two temporary relations.
    TempSwap {
        left: CompactString,
        right: CompactString,
    },
    /// Debug-print a temporary relation.
    TempDebug { temp: CompactString },
}
```

> A series of `{}` queries possibly with imperative directives like `%if` and `%loop`.
```rust
pub type ImperativeProgram = Vec<ImperativeStmt>;
```

```rust
pub struct SourceSpan(pub usize, pub usize);
```

> Parse a text script into the datalog AST.
>
> * `src` - the script to parse
> * `param_pool` - the list of parameters to execute the script with. These are substituted into the syntax tree during parsing.
> * `fixed_rules` - a mapping of fixed rule names to their implementations. These are substituted into the syntax tree during parsing.
> * `cur_vld` - the current timestamp, substituted into expressions where validity is relevant.
>
> # Errors
>
> Returns an error if the source contains syntax errors or if parsing fails.
```rust
pub fn parse_script (
    src: &str,
    param_pool: &BTreeMap<String, DataValue>,
    fixed_rules: &BTreeMap<String, Arc<Box<dyn FixedRule>>>,
    cur_vld: ValidityTs,
) -> Result<DatalogScript>
```

## `src/parse/sys/mod.rs`

```rust
pub enum SysOp {
    /// Trigger storage compaction.
    Compact,
    /// List columns of a relation.
    ListColumns(Symbol),
    /// List indices on a relation.
    ListIndices(Symbol),
    /// List all relations.
    ListRelations,
    /// List running queries.
    ListRunning,
    /// List registered fixed rules.
    ListFixedRules,
    /// Kill a running query by process ID.
    KillRunning(u64),
    /// Explain the query plan for a program.
    Explain(Box<InputProgram>),
    /// Remove one or more relations.
    RemoveRelation(Vec<Symbol>),
    /// Rename relations: `(old_name, new_name)` pairs.
    RenameRelation(Vec<(Symbol, Symbol)>),
    /// Show triggers on a relation.
    ShowTrigger(Symbol),
    /// Set triggers on a relation: `(put_scripts, rm_scripts, replace_scripts)`.
    SetTriggers(Symbol, Vec<String>, Vec<String>, Vec<String>),
    /// Set access level on one or more relations.
    SetAccessLevel(Vec<Symbol>, AccessLevel),
    /// Create a standard (B-tree) index.
    CreateIndex(Symbol, Symbol, Vec<Symbol>),
    /// Create an HNSW vector similarity index.
    CreateVectorIndex(HnswIndexConfig),
    /// Create a full-text search index.
    CreateFtsIndex(FtsIndexConfig),
    /// Create a MinHash LSH index.
    CreateMinHashLshIndex(MinHashLshConfig),
    /// Remove an index.
    RemoveIndex(Symbol, Symbol),
    /// Set a description on a relation.
    DescribeRelation(Symbol, CompactString),
}
```

```rust
pub struct FtsIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
}
```

```rust
pub struct MinHashLshConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub extractor: String,
    pub tokenizer: TokenizerConfig,
    pub filters: Vec<TokenizerConfig>,
    pub n_gram: usize,
    pub n_perm: usize,
    pub false_positive_weight: OrderedFloat<f64>,
    pub false_negative_weight: OrderedFloat<f64>,
    pub target_threshold: OrderedFloat<f64>,
}
```

```rust
pub struct HnswIndexConfig {
    pub base_relation: CompactString,
    pub index_name: CompactString,
    pub vec_dim: usize,
    pub dtype: VecElementType,
    pub vec_fields: Vec<CompactString>,
    pub distance: HnswDistance,
    pub ef_construction: usize,
    pub m_neighbours: usize,
    pub index_filter: Option<String>,
    pub extend_candidates: bool,
    pub keep_pruned_connections: bool,
}
```

```rust
pub enum HnswDistance {
    /// Euclidean (L2) distance.
    L2,
    /// Inner product distance.
    InnerProduct,
    /// Cosine distance.
    Cosine,
}
```

## `src/query_cache.rs`

```rust
pub struct QueryCacheStats {
    /// Number of cache hits since the cache was created.
    pub hits: u64,
    /// Number of cache misses since the cache was created.
    pub misses: u64,
    /// Maximum number of distinct normalized queries the cache can hold.
    pub capacity: usize,
    /// Number of distinct normalized queries currently held in the cache.
    pub len: usize,
}
```

> LRU-bounded cache for Datalog query strings.
>
> On each [`QueryCache::check`] call the query is normalized (whitespace
> collapsed), then looked up in an LRU cache.  A hit promotes the entry to
> the most-recently-used position and increments the hit counter; a miss
> inserts the entry and increments the miss counter.
>
> The cache does not store compiled query plans -- it tracks *which queries
> have been seen* and exposes hit/miss metrics so callers can observe query
> repetition patterns and make caching decisions from the metrics.
```rust
pub struct QueryCache {
    inner: Mutex<LruCache<String, ()>>,
    hits: AtomicU64,
    misses: AtomicU64,
    capacity: NonZeroUsize,
}
```

```rust
impl QueryCache {
    pub fn new (capacity: NonZeroUsize) -> Self;
    pub fn normalize (query: &str) -> String;
    pub fn check (&self, query: &str) -> bool;
    pub fn stats (&self) -> QueryCacheStats;
}
```

## `src/runtime/callback.rs`

```rust
pub enum CallbackOp {
    /// Triggered by Put operations
    Put,
    /// Triggered by Rm operations
    Rm,
}
```

```rust
impl CallbackOp {
    pub fn as_str (&self) -> &'static str;
}
```

```rust
pub struct CallbackDeclaration {
    pub(crate) dependent: CompactString,
    pub(crate) sender: Sender<(CallbackOp, NamedRows, NamedRows)>,
}
```

## `src/runtime/db.rs`

```rust
pub enum ScriptMutability {
    /// The script is mutable.
    Mutable,
    /// The script is immutable.
    Immutable,
}
```

```rust
pub struct Db<S> {
    pub(crate) db: S,
    pub(crate) temp_db: TempStorage,
    pub(crate) relation_store_id: Arc<AtomicU64>,
    pub(crate) queries_count: Arc<AtomicU64>,
    /// Guards the set of in-flight queries. Invariant: each running query has
    /// exactly one entry keyed by its monotonic id; the entry is removed on
    /// completion or cancellation. Held briefly during query start, kill, and cleanup.
    pub(crate) running_queries: Arc<Mutex<BTreeMap<u64, RunningQueryHandle>>>,
    pub(crate) fixed_rules: Arc<ShardedLock<BTreeMap<String, Arc<Box<dyn FixedRule>>>>>,
    pub(crate) tokenizers: Arc<TokenizerCache>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) callback_count: Arc<AtomicU32>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) event_callbacks: Arc<ShardedLock<EventCallbackRegistry>>,
    pub(crate) relation_locks: Arc<ShardedLock<BTreeMap<CompactString, Arc<ShardedLock<()>>>>>,
    #[cfg(feature = "hot-reload")]
    pub(crate) rule_store: Option<Arc<arc_swap::ArcSwap<crate::hot_reload::RuleSet>>>,
}
```

> Rows in a relation, together with headers for the fields.
```rust
pub struct NamedRows {
    /// The headers
    pub headers: Vec<String>,
    /// The rows
    pub rows: Vec<Tuple>,
    /// Contains the next named rows, if exists
    pub next: Option<Box<NamedRows>>,
}
```

```rust
impl NamedRows {
    pub fn new (headers: Vec<String>, rows: Vec<Tuple>) -> Self;
    pub fn has_more (&self) -> bool;
    pub fn flatten (self) -> Vec<Self>;
    pub fn into_json (self) -> JsonValue;
    pub fn from_json (value: &JsonValue) -> Result<Self>;
    pub fn into_payload (self, relation: &str, op: &str) -> Payload;
}
```

> The query and parameters.
```rust
pub type Payload = (String, BTreeMap<String, DataValue>);
```

```rust
pub enum TransactionPayload {
    /// Commit the current transaction
    Commit,
    /// Abort the current transaction
    Abort,
    /// Run a query inside the transaction
    Query(Payload),
}
```

```rust
impl <'s, S: Storage<'s>> Db<S> {
    pub fn new (storage: S) -> Result<Self>;
    pub fn initialize (&'s self) -> Result<()>;
    pub fn get_fixed_rules (&'s self) -> BTreeMap<String, Arc<Box<dyn FixedRule>>>;
    pub fn backup_db (&'s self, _out_file: impl AsRef<Path>) -> Result<()>;
    pub fn restore_backup (&'s self, _in_file: impl AsRef<Path>) -> Result<()>;
    pub fn import_from_backup (
        &'s self,
        _in_file: impl AsRef<Path>,
        _relations: &[String],
    ) -> Result<()>;
    pub fn register_fixed_rule <R> (&self, name: String, rule_impl: R) -> Result<()> where
        R: FixedRule + 'static,;
    pub fn unregister_fixed_rule (&self, name: &str) -> Result<bool>;
    pub fn register_callback (
        &self,
        relation: &str,
        capacity: Option<usize>,
    ) -> (u32, Receiver<(CallbackOp, NamedRows, NamedRows)>);
    pub fn unregister_callback (&self, id: u32) -> bool;
}
```

```rust
pub struct Poison(pub(crate) Arc<AtomicBool>);
```

```rust
impl Poison {
    pub fn check (&self) -> Result<()>;
}
```

## `src/runtime/exec.rs`

```rust
impl <'s, S: Storage<'s>> Db<S> {
    pub fn run_script (
        &'s self,
        payload: &str,
        params: BTreeMap<String, DataValue>,
        mutability: ScriptMutability,
    ) -> Result<NamedRows>;
    pub fn run_script_read_only (
        &'s self,
        payload: &str,
        params: BTreeMap<String, DataValue>,
    ) -> Result<NamedRows>;
    pub fn run_script_ast (
        &'s self,
        payload: DatalogScript,
        cur_vld: ValidityTs,
        mutability: ScriptMutability,
    ) -> Result<NamedRows>;
}
```

## `src/runtime/minhash_lsh.rs`

```rust
impl LshParams {
    pub fn find_optimal_params (threshold: f64, num_perm: usize, weights: &Weights) -> LshParams;
}
```

## `src/runtime/relation/handles.rs`

```rust
pub enum AccessLevel {
    Hidden,
    ReadOnly,
    Protected,
    #[default]
    Normal,
}
```

```rust
pub fn decode_tuple_from_kv (key: &[u8], val: &[u8], size_hint: Option<usize>) -> Tuple
```

```rust
pub fn extend_tuple_from_v (key: &mut Tuple, val: &[u8])
```

## `src/runtime/temp_store.rs`

```rust
pub struct RegularTempStore {
    inner: BTreeMap<Tuple, bool>,
}
```

```rust
impl RegularTempStore {
    pub fn exists (&self, key: &Tuple) -> bool;
    pub fn put (&mut self, tuple: Tuple);
}
```

## `src/runtime/transact.rs`

> A transaction session binding a storage transaction and a temporary store.
>
> Dropping without calling [`commit_tx`](Self::commit_tx) implicitly aborts.
> All schema mutations (create/destroy relations, set triggers, etc.) go
> through this handle.
```rust
pub struct SessionTx<'a> {
    pub(crate) store_tx: Box<dyn StoreTx<'a> + 'a>,
    pub(crate) temp_store_tx: TempTx,
    pub(crate) relation_store_id: Arc<AtomicU64>,
    pub(crate) temp_store_id: AtomicU32,
    pub(crate) tokenizers: Arc<TokenizerCache>,
}
```

```rust
pub const CURRENT_STORAGE_VERSION: [u8; 1] = [0x00];
```

```rust
impl SessionTx<'_> {
    pub fn commit_tx (&mut self) -> Result<()>;
}
```

```rust
impl <'s, S: Storage<'s>> Db<S> {
    pub fn run_multi_transaction (
        &'s self,
        is_write: bool,
        payloads: Receiver<TransactionPayload>,
        results: Sender<Result<NamedRows>>,
    );
    pub fn export_relations <I, T> (&'s self, relations: I) -> Result<BTreeMap<String, NamedRows>> where
        T: AsRef<str>,
        I: Iterator<Item = T>,;
    pub fn import_relations (&'s self, data: BTreeMap<String, NamedRows>) -> Result<()>;
}
```

## `src/storage/error.rs`

```rust
pub enum StorageError {
    /// A storage backend operation failed (e.g., `begin_write`, `open_table`, commit).
    #[snafu(display("transaction failed ({backend}): {message}"))]
    TransactionFailed {
        backend: &'static str,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Attempted a write operation on a read-only transaction.
    #[snafu(display("write attempted on a read-only transaction"))]
    WriteInReadTransaction {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Data corruption detected in storage.
    #[snafu(display("corrupted data: {message}"))]
    CorruptedData {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage I/O error (e.g., creating directories, reading files).
    #[snafu(display("storage I/O error ({backend}): {source}"))]
    Io {
        backend: &'static str,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Key encoding/decoding error.
    #[snafu(display("key encoding error: {message}"))]
    KeyEncoding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/storage/fjall_backend.rs`

```rust
pub fn new_krites_fjall (
    path: impl AsRef<Path>,
) -> crate::error::InternalResult<DbCore<FjallStorage>>
```

```rust
pub struct FjallStorage {
    db: Arc<fjall::SingleWriterTxDatabase>,
    keyspace: Arc<fjall::SingleWriterTxKeyspace>,
}
```

```rust
pub enum FjallTx<'s> {
    Reader(FjallReadTx<'s>),
    Writer(Box<FjallWriteTx<'s>>),
}
```

```rust
pub struct FjallReadTx<'s> {
    snapshot: fjall::Snapshot,
    keyspace: &'s fjall::SingleWriterTxKeyspace,
}
```

```rust
pub struct FjallWriteTx<'s> {
    tx: Option<fjall::SingleWriterWriteTx<'s>>,
    keyspace: &'s fjall::SingleWriterTxKeyspace,
}
```

## `src/storage/mem.rs`

```rust
pub fn new_mem_db () -> crate::error::InternalResult<crate::DbCore<MemStorage>>
```

```rust
pub struct MemStorage {
    store: Arc<ShardedLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}
```

```rust
pub enum MemTx<'s> {
    Reader(ShardedLockReadGuard<'s, BTreeMap<Vec<u8>, Vec<u8>>>),
    Writer(
        ShardedLockWriteGuard<'s, BTreeMap<Vec<u8>, Vec<u8>>>,
        BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    ),
}
```

## `src/storage/mod.rs`

```rust
pub trait Storage <'s> : Send + Sync + Clone {
    fn storage_kind (&self) -> &'static str;
    fn transact (&'s self, write: bool) -> StorageResult<Self::Tx>;
    fn range_compact (&'s self, lower: &[u8], upper: &[u8]) -> StorageResult<()>;
    fn batch_put <'a> (
        &'a self,
        data: Box<dyn Iterator<Item = StorageResult<(Vec<u8>, Vec<u8>)>> + 'a>,
    ) -> StorageResult<()>;
}
```

> Trait for the associated transaction type of a storage engine.
> A transaction needs to guarantee MVCC semantics for all operations.
```rust
pub trait StoreTx <'s> : Sync {
    fn get (&self, key: &[u8], for_update: bool) -> StorageResult<Option<Vec<u8>>>;
    fn put (&mut self, key: &[u8], val: &[u8]) -> StorageResult<()>;
    fn supports_par_put (&self) -> bool;
    fn par_put (&self, _key: &[u8], _val: &[u8]) -> StorageResult<()>; // default impl
    fn del (&mut self, key: &[u8]) -> StorageResult<()>;
    fn del_range_from_persisted (&mut self, lower: &[u8], upper: &[u8]) -> StorageResult<()>;
    fn exists (&self, key: &[u8], for_update: bool) -> StorageResult<bool>;
    fn commit (&mut self) -> StorageResult<()>;
    fn range_scan_tuple <'a> (
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a> where
        's: 'a,; // default impl
    fn range_skip_scan_tuple <'a> (
        &'a self,
        lower: &[u8],
        upper: &[u8],
        valid_at: ValidityTs,
    ) -> Box<dyn Iterator<Item = InternalResult<Tuple>> + 'a>;
    fn range_scan <'a> (
        &'a self,
        lower: &[u8],
        upper: &[u8],
    ) -> Box<dyn Iterator<Item = InternalResult<(Vec<u8>, Vec<u8>)>> + 'a> where
        's: 'a;
    fn range_count <'a> (&'a self, lower: &[u8], upper: &[u8]) -> StorageResult<usize> where
        's: 'a;
}
```
