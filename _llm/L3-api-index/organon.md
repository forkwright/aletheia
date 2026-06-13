# L3 API Index: organon

Crate path: `crates/organon`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/builtins/computer_use/executor.rs`

> Register the `computer_use` tool into the registry.
> 
> Uses the provided [`SandboxConfig`] to derive default session
> sandbox policy. The tool is registered with `auto_activate: false`,
> requiring explicit activation via `enable_tool`.
> 
> # Errors
> 
> Returns an error if the tool name collides with an existing tool.
```rust
pub fn register (registry: &mut ToolRegistry, sandbox: &SandboxConfig) -> Result<()>
```

## `src/builtins/energeia/mod.rs`

> Register all 9 energeia tools.
> 
> When `services` is `Some`, tools that need the orchestrator or store call
> through to the real energeia subsystem. When `None`, those tools return a
> structured error indicating the missing dependency  -  they do not panic.
> 
> Tools that are bounded local computations (`schedion`, `prographe`,
> `diorthosis`, `dokimasia`, `epitropos`) work regardless of whether services
> are provided, but their public definitions describe the current limitations.
> 
> # Errors
> 
> Returns an error if any tool name collides with an already-registered tool.
```rust
pub fn register (registry: &mut ToolRegistry, services: Option<&EnergeiaServices>) -> Result<()>
```

## `src/builtins/energeia/shared.rs`

> Services injected at registration time for energeia tool executors.
> 
> The orchestrator handles dispatch (dromeus), and the store backs lessons,
> observations, and metrics (mathesis, parateresis, metron, diorthosis).
```rust
pub struct EnergeiaServices {
    /// Top-level dispatch orchestrator wiring engine, QA, and store.
    pub orchestrator: Arc<Orchestrator>,
    /// State persistence store for lessons, observations, and CI validations.
    pub store: Arc<EnergeiaStore>,
}
```

```rust
impl EnergeiaServices {
    pub fn new (orchestrator: Arc<Orchestrator>, store: Arc<EnergeiaStore>) -> Self;
}
```

## `src/builtins/mod.rs`

> Register all built-in tool executors with default sandbox config.
> 
> # Errors
> 
> Returns an error if any built-in tool name collides with an
> already-registered tool.
```rust
pub fn register_all (registry: &mut ToolRegistry) -> Result<()>
```

> Register all built-in tool executors with custom sandbox config.
> 
> Registration is two-phase:
> 
> 1. All domain tools are registered first.
> 2. `tool_schema` is registered last, capturing a schema snapshot of every
>    tool registered in phase 1.  This avoids a self-referential ownership
>    cycle (the registry owns the `tool_schema` executor, which cannot safely
>    hold a back-reference to the same registry).
> 
> Callers that register additional tools after this function (for example
> domain packs or external HTTP/MCP tools) should call
> [`ToolRegistry::finalize_tool_schema`] to refresh the snapshot with the
> complete tool set.
> 
> # Errors
> 
> Returns an error if any built-in tool name collides with an
> already-registered tool.
```rust
pub fn register_all_with_sandbox (
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
) -> Result<()>
```

```rust
pub fn register_all_with_sandbox_and_energeia_services (
    registry: &mut ToolRegistry,
    sandbox: SandboxConfig,
    services: &energeia::EnergeiaServices,
) -> Result<()>
```

## `src/builtins/skill_read.rs`

> Register the `skill_read` tool into `registry`.
> 
> # Errors
> 
> Returns an error if `skill_read` is already registered.
```rust
pub fn register (registry: &mut ToolRegistry) -> Result<()>
```

## `src/builtins/working_checkpoint.rs`

```rust
pub enum WorkingCheckpointScope {
    /// Session-scoped checkpoint (default).
    #[default]
    Session,
}
```

```rust
pub struct UpdateWorkingCheckpointInput {
    /// Structured `key_info` content the agent has decided is worth retaining.
    pub content: String,
    /// Scope of the checkpoint. Currently "session" only; "project" follow-up.
    #[serde(default)]
    pub scope: WorkingCheckpointScope,
}
```

> Register the `update_working_checkpoint` tool into `registry`.
> 
> # Errors
> 
> Returns an error if the tool name collides with an already-registered tool.
```rust
pub fn register (registry: &mut ToolRegistry) -> Result<()>
```

## `src/error.rs`

```rust
pub enum Error {
    /// Requested tool does not exist in the registry.
    #[snafu(display("tool not found: {name}"))]
    ToolNotFound {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A tool with this name is already registered.
    #[snafu(display("duplicate tool: {name}"))]
    DuplicateTool {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool input failed validation.
    #[snafu(display("invalid input for tool {name}: {reason}"))]
    InvalidInput {
        name: ToolName,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool execution returned an error.
    #[snafu(display("tool execution failed: {name}: {message}"))]
    ExecutionFailed {
        name: ToolName,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize an input schema to JSON.
    #[snafu(display("schema serialization failed"))]
    SchemaSerialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// File-ref interpolation failed during tool argument expansion.
    #[snafu(display("file-ref interpolation failed: {source}"))]
    InterpError {
        source: crate::interp::InterpError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool call rejected because the tool's groups do not intersect the role's allowed groups.
    #[snafu(display(
        "tool group violation: role {role} cannot call tool {tool}: allowed groups {allowed:?}, tool groups {tool_groups:?}"
    ))]
    ToolGroupViolation {
        role: String,
        tool: String,
        allowed: Vec<crate::types::ToolGroupId>,
        tool_groups: Vec<crate::types::ToolGroupId>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// `tool_schema` has not been registered yet.
    #[snafu(display("tool_schema is not registered"))]
    ToolSchemaNotRegistered {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Lock guarding the `tool_schema` snapshot was poisoned.
    #[snafu(display("tool_schema snapshot lock poisoned"))]
    SchemaSnapshotPoisoned {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

> Convenience alias.
```rust
pub type Result<T> = std::result::Result<T, Error>;
```

```rust
pub enum StoreError {
    // kanon:ignore RUST/pub-visibility
    /// The requested entity was not found.
    #[snafu(display("{entity} not found: {id}"))]
    StoreNotFound { entity: String, id: String },

    /// A conflicting entry already exists.
    #[snafu(display("{entity} conflict: {id}"))]
    StoreConflict { entity: String, id: String },

    /// An I/O error occurred during a store operation.
    #[snafu(display("store I/O error: {context}"))]
    StoreIo {
        context: String,
        source: std::io::Error,
    },

    /// Serialization or deserialization failed.
    #[snafu(display("store serialization error"))]
    StoreSerialization { source: serde_json::Error },

    /// A backend-specific error that doesn't fit other variants.
    #[snafu(display("store backend error: {message}"))]
    Backend { message: String },
}
```

```rust
pub enum PlanningAdapterError {
    // kanon:ignore RUST/pub-visibility
    #[snafu(display("failed to access workspace: {message}"))]
    Workspace {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to load project: {message}"))]
    LoadProject {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to save project: {message}"))]
    SaveProject {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to serialize project: {source}"))]
    Serialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("state transition failed: {message}"))]
    Transition {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unknown project mode: {mode}"))]
    InvalidMode {
        mode: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unknown transition: {name}"))]
    InvalidTransition {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid {kind}: {message}"))]
    InvalidId {
        kind: String,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("{kind} not found: {id}"))]
    NotFound {
        kind: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("background task panicked: {source}"))]
    TaskJoin {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("filesystem error: {source}"))]
    Io {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

```rust
pub enum KnowledgeAdapterError {
    // kanon:ignore RUST/pub-visibility
    #[snafu(display("embedding failed: {message}"))]
    Embedding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("search failed: {message}"))]
    Search {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fact query failed: {message}"))]
    FactQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("store mutation failed: {message}"))]
    MutateStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("datalog query failed: {message}"))]
    DatalogQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid forget reason: {reason}"))]
    InvalidReason {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/interp.rs`

```rust
pub enum InterpError {
    /// The requested file does not exist.
    #[snafu(display("file not found: {}", path.display()))]
    FileNotFound { path: PathBuf },

    /// The requested line range exceeds the file's actual line count.
    #[snafu(display(
        "line range {requested_start}..{requested_end} out of bounds; file has {actual_lines} lines: {}",
        path.display()
    ))]
    OutOfBounds {
        path: PathBuf,
        requested_start: usize,
        requested_end: usize,
        actual_lines: usize,
    },

    /// Absolute paths are rejected by default.
    #[snafu(display("absolute path not allowed: {}", path.display()))]
    AbsolutePathRejected { path: PathBuf },

    /// An I/O error occurred while reading the file.
    #[snafu(display("io error reading {}: {source}", path.display()))]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// A line number in the template could not be parsed.
    #[snafu(display("invalid line number in template: {value}"))]
    InvalidLineNumber { value: String },
}
```

```rust
pub fn expand_file_refs (text: &str, workspace_root: &Path) -> Result<String, InterpError>
```

> Recursively expand file refs in every JSON string value.
> 
> Objects and arrays are traversed depth-first. Non-string values are cloned
> unchanged.
> 
> # Errors
> 
> Returns [`InterpError`] on the first file-ref that fails to resolve.
```rust
pub fn expand_file_refs_in_json (
    value: &serde_json::Value,
    workspace_root: &Path,
) -> Result<serde_json::Value, InterpError>
```

## `src/metrics.rs`

```rust
pub struct LiveInvocation {
    /// Stable invocation identifier.
    pub id: u64,
    /// Tool name being executed.
    pub tool_name: String,
    /// When the invocation started.
    pub started_at: Instant,
}
```

```rust
pub struct ActiveInvocationGuard {
    id: u64,
}
```

```rust
pub fn track_invocation (tool_name: &str) -> ActiveInvocationGuard
```

```rust
pub fn live_invocations () -> Vec<LiveInvocation>
```

> Register this crate's metrics with the shared registry.
```rust
pub fn register (registry: &mut Registry)
```

## `src/process_guard.rs`

> RAII guard that kills and reaps a child process on drop.
> 
> Drop calls `kill()` followed by `wait()` on the inner
> [`Child`][std::process::Child].  Both calls ignore errors: `kill()` fails
> if the process has already exited (safe), and `wait()` fails if the OS
> has already reaped the zombie (safe).
```rust
pub struct ProcessGuard {
    child: Option<std::process::Child>,
}
```

```rust
impl ProcessGuard {
    pub fn new (child: std::process::Child) -> Self;
    pub fn get_mut (&mut self) -> &mut std::process::Child;
}
```

## `src/receipts.rs`

```rust
pub struct ReceiptSigner {
    key: [u8; 32],
}
```

```rust
impl ReceiptSigner {
    pub fn new_session () -> Self;
    pub fn sign (
        &self,
        tool_name: &str,
        args_json: &str,
        result: &str,
        ts: jiff::Timestamp,
    ) -> String;
    pub fn verify (
        &self,
        receipt: &str,
        tool_name: &str,
        args_json: &str,
        result: &str,
        ts: jiff::Timestamp,
    ) -> Result<(), VerifyError>;
}
```

```rust
pub struct ReceiptLedger {
    entries: Vec<EmittedReceipt>,
}
```

```rust
pub struct EmittedReceipt {
    /// The receipt token (base64url, no padding).
    pub receipt: String,
    /// Tool name.
    pub tool_name: String,
    /// Arguments JSON at emission time.
    pub args_json: String,
    /// Result text at emission time.
    pub result: String,
    /// Timestamp used for signing.
    pub ts: jiff::Timestamp,
}
```

```rust
impl EmittedReceipt {
    pub fn new (
        receipt: String,
        tool_name: String,
        args_json: String,
        result: String,
        ts: jiff::Timestamp,
    ) -> Self;
}
```

```rust
impl ReceiptLedger {
    pub fn record (
        &mut self,
        receipt: String,
        tool_name: String,
        args_json: String,
        result: String,
        ts: jiff::Timestamp,
    );
    pub fn lookup (&self, receipt: &str) -> Option<&EmittedReceipt>;
}
```

> Scan an assistant message for cited receipts and verify each against the ledger.
> 
> # Errors
> Returns [`HallucinationDetected::HallucinatedReceipt`] if a cited receipt is
> not present in the ledger, or [`HallucinationDetected::ReceiptInvalid`] if
> verification fails (e.g. HMAC mismatch).
```rust
pub fn scan_and_verify (
    signer: &ReceiptSigner,
    ledger: &ReceiptLedger,
    assistant_text: &str,
) -> Result<(), HallucinationDetected>
```

```rust
pub enum VerifyError {
    /// Receipt missing or malformed (not valid base64url).
    #[snafu(display("receipt missing or malformed"))]
    Malformed,
    /// HMAC mismatch — receipt does not authenticate this tuple.
    #[snafu(display("HMAC mismatch — receipt does not authenticate this tuple"))]
    HmacMismatch,
    /// Base64 decode error.
    #[snafu(display("decode error: {source}"))]
    Decode {
        /// Underlying base64 error.
        source: base64::DecodeError,
    },
}
```

```rust
pub enum HallucinationDetected {
    /// Model cited a receipt not present in the ledger — fabricated tool call.
    #[snafu(display("model cited receipt {receipt} not present in ledger — fabricated tool call"))]
    HallucinatedReceipt {
        /// The receipt token cited by the model.
        receipt: String,
    },
    /// Receipt present in ledger but verification failed.
    #[snafu(display("receipt {receipt} verification failed: {source}"))]
    ReceiptInvalid {
        /// The receipt token.
        receipt: String,
        /// Underlying verification error.
        source: VerifyError,
    },
}
```

## `src/registry/mod.rs`

> The trait tool implementations must satisfy.
> 
> Uses `Pin<Box<dyn Future>>` for object-safety with async dispatch.
> 
> # Errors
> 
> Implementations may return `ExecutionFailed` if the tool
> cannot complete its operation, or `InvalidInput` if the
> provided arguments fail validation.
> 
> # Examples
> 
> ```no_run
> use std::future::Future;
> use std::pin::Pin;
> use organon::registry::ToolExecutor;
> use organon::types::{ToolContext, ToolInput, ToolResult};
> 
> struct MyTool;
> 
> impl ToolExecutor for MyTool {
>     fn execute<'a>(
>         &'a self,
>         _input: &'a ToolInput,
>         _ctx: &'a ToolContext,
>     ) -> Pin<Box<dyn Future<Output = organon::error::Result<ToolResult>> + Send + 'a>> {
>         Box::pin(async move { Ok(ToolResult::text("done")) })
>     }
> }
> ```
```rust
pub trait ToolExecutor : Send + Sync {
    fn execute <'a> (
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}
```

> Registry of available tools.
> 
> Tools are registered at startup and looked up by name during execution.
> The registry is the single source of truth for what tools an agent can use.
> 
> # Examples
> 
> ```no_run
> use organon::registry::ToolRegistry;
> 
> let mut registry = ToolRegistry::new();
> // Tools are registered at startup with their definitions and executors.
> // See the `builtins` module for built-in tool implementations.
> ```
```rust
pub struct ToolRegistry {
    // kanon:ignore RUST/pub-visibility
    tools: IndexMap<ToolName, RegisteredTool>,
    /// Snapshot state for the `tool_schema` meta-tool.  `None` until
    /// `tool_schema` is registered.
    tool_schema_snapshot: Option<ToolSchemaSnapshot>,
}
```

```rust
impl ToolRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, def: ToolDef, executor: Box<dyn ToolExecutor>) -> Result<()>;
    pub fn register_with_call_capability (
        &mut self,
        def: ToolDef,
        call_capability: ToolCallCapabilityRule,
        executor: Box<dyn ToolExecutor>,
    ) -> Result<()>;
    pub fn get_def (&self, name: &ToolName) -> Option<&ToolDef>;
    pub async fn execute (&self, input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult>;
    pub async fn execute_checked (
        &self,
        input: &ToolInput,
        ctx: &ToolContext,
        role: &str,
        policy: &ToolGroupPolicy,
    ) -> Result<ToolResult>;
    pub fn definitions (&self) -> Vec<&ToolDef>;
    pub fn definitions_for_category (&self, category: ToolCategory) -> Vec<&ToolDef>;
    pub fn definitions_for_tags (&self, tags: &[ToolTag]) -> Vec<&ToolDef>;
    pub fn definitions_for_groups (&self, allowed_groups: &[ToolGroupId]) -> Vec<&ToolDef>;
    pub fn definitions_for_policy (&self, policy: &ToolGroupPolicy) -> Vec<&ToolDef>;
    pub fn to_hermeneus_tools (&self) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_for_groups (
        &self,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_for_policy (
        &self,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries (&self) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries_for_groups (
        &self,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries_for_policy (
        &self,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries_filtered (
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries_filtered_for_groups (
        &self,
        active: &HashSet<ToolName>,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_summaries_filtered_for_policy (
        &self,
        active: &HashSet<ToolName>,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn schema_byte_sizes (&self) -> (usize, usize);
    pub fn to_hermeneus_tools_filtered (
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_filtered_for_groups (
        &self,
        active: &HashSet<ToolName>,
        allowed_groups: &[ToolGroupId],
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_filtered_for_policy (
        &self,
        active: &HashSet<ToolName>,
        policy: &ToolGroupPolicy,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn reversibility (&self, name: &ToolName) -> Option<Reversibility>;
    pub fn approval_requirement (&self, name: &ToolName) -> Option<ApprovalRequirement>;
    pub fn call_capability (&self, input: &ToolInput) -> Result<ToolCallCapability>;
    pub fn permits_call (&self, input: &ToolInput, policy: &ToolGroupPolicy) -> Result<bool>;
    pub fn approval_requirement_for_input (&self, input: &ToolInput) -> Result<ApprovalRequirement>;
    pub fn call_metadata (&self, name: &ToolName, dry_run: bool) -> Option<ToolCallMetadata>;
    pub fn call_metadata_for_input (
        &self,
        input: &ToolInput,
        dry_run: bool,
    ) -> Result<ToolCallMetadata>;
    pub fn lazy_tool_catalog (&self) -> Vec<(ToolName, String)>;
    pub fn finalize_tool_schema (&mut self) -> Result<()>;
    pub fn is_daemon_safe (&self, name: &ToolName) -> bool;
    pub fn daemon_safe_tools (&self) -> Vec<&ToolDef>;
}
```

## `src/sandbox/config.rs`

```rust
pub enum SandboxEnforcement {
    /// Sandbox violations cause the operation to fail.
    Enforcing,
    /// Sandbox violations are logged but allowed to proceed.
    Permissive,
}
```

```rust
pub enum EgressPolicy {
    /// Block all outbound network from child processes.
    Deny,
    /// No egress filtering; child processes have full network access.
    #[default]
    Allow,
    /// Permit only connections to listed destinations.
    Allowlist,
}
```

```rust
pub struct SandboxConfig {
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcement,
    /// Default filesystem root granted read access.
    ///
    /// Defaults to `~` which expands to the HOME environment variable at
    /// policy-build time. Operators can set this to a stricter path to
    /// prevent agents from reading files outside a specific directory.
    ///
    /// WHY: without a home-directory default, agents cannot read user files
    /// (dotfiles, project repos, etc.) even in permissive mode.
    pub allowed_root: PathBuf,
    /// Additional filesystem paths granted read access.
    pub extra_read_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted read+write access.
    pub extra_write_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted execute access.
    ///
    /// Values may begin with `~` which is expanded to the HOME environment
    /// variable at policy-build time.
    pub extra_exec_paths: Vec<PathBuf>,
    /// Network egress policy for child processes.
    pub egress: EgressPolicy,
    /// Addresses or CIDR ranges permitted when `egress = "allowlist"`.
    ///
    /// Entries are parsed as IP addresses or CIDR notation (e.g.
    /// `"127.0.0.1"`, `"::1"`, `"10.0.0.0/8"`). Only loopback
    /// destinations can be enforced without root privileges; non-loopback
    /// entries log a warning.
    pub egress_allowlist: Vec<String>,
    /// Maximum number of processes (`RLIMIT_NPROC`) for exec child processes.
    ///
    /// WHY: `RLIMIT_NPROC` counts ALL processes for the user, not just sandbox
    /// children. The previous default of 64 caused EAGAIN failures on systems
    /// running dispatch agents or other background processes. Default: 256.
    pub nproc_limit: u32,
}
```

```rust
pub struct SandboxPolicy {
    /// Whether sandbox restrictions are applied at all.
    ///
    /// When `false`, `apply_sandbox` returns immediately without registering
    /// any `pre_exec` hook. Callers need not check this field separately.
    pub enabled: bool,
    /// Filesystem paths granted read access.
    pub read_paths: Vec<PathBuf>,
    /// Filesystem paths granted read+write access.
    pub write_paths: Vec<PathBuf>,
    /// Filesystem paths granted execute access.
    pub exec_paths: Vec<PathBuf>,
    /// Enforcement level.
    pub enforcement: SandboxEnforcement,
    /// Network egress policy.
    pub egress: EgressPolicy,
    /// Allowed destinations when `egress == Allowlist`.
    pub egress_allowlist: Vec<String>,
}
```

```rust
impl SandboxConfig {
    pub fn build_policy (&self, workspace: &Path, allowed_roots: &[PathBuf]) -> SandboxPolicy;
}
```

## `src/sandbox/policy.rs`

```rust
pub fn probe_landlock_abi () -> Option<i32>
```

```rust
pub fn probe_landlock_abi () -> Option<i32>
```

```rust
pub fn apply_sandbox (
    // kanon:ignore RUST/pub-visibility
    cmd: &mut std::process::Command,
    policy: SandboxPolicy,
) -> std::io::Result<()>
```

```rust
pub fn apply_sandbox (
    // kanon:ignore RUST/pub-visibility
    _cmd: &mut std::process::Command,
    policy: SandboxPolicy,
) -> std::io::Result<()>
```

## `src/subprocess.rs`

```rust
pub struct SubprocessRequest {
    program: OsString,
    args: Vec<OsString>,
    current_dir: PathBuf,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
    max_output_bytes: usize,
    extra_read_paths: Vec<PathBuf>,
    extra_write_paths: Vec<PathBuf>,
    extra_exec_paths: Vec<PathBuf>,
}
```

```rust
impl SubprocessRequest {
    pub fn new (program: impl Into<OsString>, current_dir: impl Into<PathBuf>) -> Self;
    pub fn arg (mut self, arg: impl Into<OsString>) -> Self;
    pub fn args <I, S> (mut self, args: I) -> Self where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,;
    pub fn stdin_bytes (mut self, stdin: impl Into<Vec<u8>>) -> Self;
    pub fn timeout (mut self, timeout: Duration) -> Self;
    pub fn max_output_bytes (mut self, max_output_bytes: usize) -> Self;
    pub fn allow_read_path (mut self, path: impl Into<PathBuf>) -> Self;
    pub fn allow_write_path (mut self, path: impl Into<PathBuf>) -> Self;
    pub fn allow_exec_path (mut self, path: impl Into<PathBuf>) -> Self;
}
```

```rust
pub struct SubprocessOutput {
    /// Process exit code, or `-1` when the platform did not provide one.
    pub exit_code: i32,
    /// Captured stdout, bounded by the request limit.
    pub stdout: String,
    /// Captured stderr, bounded by the request limit.
    pub stderr: String,
    /// Wall-clock duration of the subprocess.
    pub duration: Duration,
}
```

```rust
pub enum SubprocessError {
    /// Sandbox setup failed before the process was spawned.
    SandboxSetup(std::io::Error),
    /// Process spawn failed.
    Spawn(std::io::Error),
    /// Writing stdin failed.
    Stdin(std::io::Error),
    /// Waiting for the process failed.
    Wait(std::io::Error),
    /// The process exceeded its wall-clock timeout.
    Timeout(Duration),
}
```

```rust
pub struct SubprocessRunner {
    sandbox: SandboxConfig,
}
```

```rust
impl SubprocessRunner {
    pub fn new (sandbox: SandboxConfig) -> Self;
    pub fn run (
        &self,
        request: SubprocessRequest,
        ctx: &ToolContext,
    ) -> Result<SubprocessOutput, SubprocessError>;
}
```

## `src/testing.rs`

> Install the default rustls crypto provider for tests.
> 
> Safe to call multiple times (uses `try_install_default`). This helper prevents
> the "no crypto provider installed" panic when tests use TLS connections.
> 
> # Example
> 
> ```ignore
> use organon::testing::install_crypto_provider;
> 
> #[test]
> fn test_with_tls() {
>     install_crypto_provider();
>     // ... test code that uses TLS
> }
> ```
```rust
pub fn install_crypto_provider ()
```

> Configurable mock [`ToolExecutor`] for use in tests.
> 
> Implements the same [`ToolExecutor`] trait as production executors.
> Supports fixed text responses, error injection, and call-count tracking.
> 
> # Examples
> 
> ```ignore
> let ex = MockToolExecutor::text("ok");
> let result = ex.execute(&input, &ctx).await.expect("execute should succeed"); // kanon:ignore RUST/expect
> assert!(!result.is_error, "result should not be an error");
> assert_eq!(ex.call_count(), 1, "call count should be 1 after one execution");
> ```
```rust
pub struct MockToolExecutor {
    name: ToolName,
    // WHY: std::sync::Mutex -- lock never held across .await
    inner: Mutex<MockInner>,
    call_count: AtomicU64,
}
```

```rust
impl MockToolExecutor {
    pub fn text (text: impl Into<String>) -> Self;
    pub fn tool_error (message: impl Into<String>) -> Self;
    pub fn sequence (results: Vec<ToolResult>) -> Self;
    pub fn named (mut self, name: ToolName) -> Self;
    pub fn name (&self) -> ToolName;
    pub fn call_count (&self) -> u64;
}
```

> Spec contract that any [`ToolExecutor`] implementation must satisfy.
> 
> Use [`ToolExecutorSpec::validate_async`] inside a `#[tokio::test]` to assert
> the contract. The report separates passed checks from failed ones so test
> output is easy to diagnose.
```rust
pub struct ToolExecutorSpec {
    tool_name: ToolName,
}
```

```rust
impl ToolExecutorSpec {
    pub fn new (tool_name: ToolName) -> Self;
    pub async fn validate_async <E: ToolExecutor> (
        &self,
        executor: &E,
        ctx: &ToolContext,
    ) -> SpecReport;
}
```

```rust
pub struct SpecReport {
    passed: Vec<String>,
    failed: Vec<(String, String)>,
}
```

```rust
impl SpecReport {
    pub fn is_passing (&self) -> bool;
    pub fn passes (&self) -> &[String];
    pub fn failures (&self) -> &[(String, String)];
}
```

```rust
pub fn make_test_context () -> ToolContext
```

```rust
pub fn make_tool_input (name: &ToolName) -> ToolInput
```

## `src/types/context.rs`

```rust
pub struct ServerToolConfig {
    /// Whether web search is available for activation.
    #[serde(default)]
    pub web_search: bool,
    /// Maximum web search uses per turn (None = provider default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search_max_uses: Option<u32>,
    /// Whether code execution is available for activation.
    #[serde(default)]
    pub code_execution: bool,
}
```

```rust
impl ServerToolConfig {
    pub fn active_definitions (
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ServerToolDefinition>;
}
```

```rust
pub struct ToolServices {
    pub cross_nous: Option<Arc<dyn CrossNousService>>,
    pub messenger: Option<Arc<dyn MessageService>>,
    pub note_store: Option<Arc<dyn NoteStore>>,
    pub blackboard_store: Option<Arc<dyn BlackboardStore>>,
    pub spawn: Option<Arc<dyn SpawnService>>,
    pub planning: Option<Arc<dyn PlanningService>>,
    pub knowledge: Option<Arc<dyn KnowledgeSearchService>>,
    pub working_checkpoint_store: Option<Arc<dyn crate::types::WorkingCheckpointStore>>,
    pub http_client: reqwest::Client,
    /// In-memory vault for session-scoped secrets (AWS SSO keys, API tokens, etc.).
    ///
    /// Referenced via `{{secret:<name>}}` or `$SECRET(<name>)` placeholders in
    /// tool arguments and substituted at dispatch time so resolved values never
    /// reach transcripts or outbound LLM payloads.
    pub secret_vault: SecretVault,
    /// Catalog of lazy tools available for activation via `enable_tool`.
    pub lazy_tool_catalog: Vec<(ToolName, String)>,
    /// Server tool configuration for provider-side tools (web search, code execution).
    pub server_tool_config: ServerToolConfig,
}
```

```rust
pub struct ToolContext {
    /// The agent executing this tool.
    pub nous_id: NousId,
    /// Current session.
    pub session_id: SessionId,
    /// Current turn number within the session.
    pub turn_number: u64,
    /// Agent workspace root.
    pub workspace: PathBuf,
    /// Allowed filesystem roots for sandboxing.
    pub allowed_roots: Vec<PathBuf>,
    /// Optional runtime services for tools that need cross-cutting capabilities.
    pub services: Option<Arc<ToolServices>>,
    /// Per-session set of dynamically activated tools (via `enable_tool`).
    pub active_tools: Arc<RwLock<HashSet<ToolName>>>,
    /// Deployment-tunable tool size and timeout limits from taxis config.
    pub tool_config: Arc<ToolLimitsConfig>,
}
```

## `src/types/mod.rs`

```rust
pub enum ToolGroupId {
    /// File/code reading tools (`read`, `grep`, `find`, `ls`, `view_file`, ...).
    Read,
    /// File/code mutation tools (`write`, `edit`, `mkdir`, `mv`, `cp`, `rm`, ...).
    Edit,
    /// Shell/cargo execution (`exec`, `git_checkout`, `computer_use`, ...).
    Command,
    /// MCP tool invocation and external API calls (`web_fetch`, `http_request`, ...).
    Mcp,
    /// Spawning sub-agents (`sessions_spawn`, `sessions_dispatch`, ...).
    SpawnSubtask,
    /// Planning and design tools (`plan_create`, `plan_roadmap`, ...).
    Plan,
    /// Tests, lint, fmt, and verification tools (`lint_report`, `verify_report`, ...).
    Verify,
}
```

```rust
pub enum ToolGroupPolicy {
    /// Every registered tool is permitted, including tools with no group metadata.
    AllowAll {
        /// Human-readable reason for granting every tool group.
        reason: String,
    },
    /// Tools are permitted when their declared groups intersect this list.
    Groups(Vec<ToolGroupId>),
    /// No grouped tools are permitted.
    #[default]
    DenyAll,
}
```

```rust
impl ToolGroupPolicy {
    pub fn groups (groups: Vec<ToolGroupId>) -> Self;
    pub fn permits (&self, tool_groups: &[ToolGroupId]) -> bool;
    pub fn allowed_groups (&self) -> &[ToolGroupId];
    pub fn description (&self) -> String;
}
```

```rust
pub struct ToolCallCapability {
    /// Tool groups required by this concrete call.
    pub groups: Vec<ToolGroupId>,
    /// Reversibility for this concrete call.
    pub reversibility: Reversibility,
}
```

```rust
impl ToolCallCapability {
    pub fn new (groups: Vec<ToolGroupId>, reversibility: Reversibility) -> Self;
}
```

```rust
pub struct ToolArgumentValueCapability {
    /// Selector value from the tool input.
    pub value: String,
    /// Capability for calls carrying this selector value.
    pub capability: ToolCallCapability,
}
```

```rust
pub enum ToolCallCapabilityRule {
    /// Classify by an argument's string value, such as `action` or `op`.
    ArgumentValue {
        /// Argument name to read from the tool input.
        argument: String,
        /// Capabilities keyed by argument value.
        values: Vec<ToolArgumentValueCapability>,
    },
    /// Classify by whether an argument is present.
    ArgumentPresence {
        /// Argument name to test in the tool input.
        argument: String,
        /// Capability when the argument is present and not null.
        present: ToolCallCapability,
        /// Capability when the argument is absent or null.
        absent: ToolCallCapability,
    },
}
```

```rust
impl ToolCallCapabilityRule {
    pub fn argument_value <V, I> (argument: impl Into<String>, values: I) -> Self where
        V: Into<String>,
        I: IntoIterator<Item = (V, ToolCallCapability)>,;
    pub fn argument_presence (
        argument: impl Into<String>,
        present: ToolCallCapability,
        absent: ToolCallCapability,
    ) -> Self;
    pub fn classify (
        &self,
        arguments: &serde_json::Value,
    ) -> std::result::Result<ToolCallCapability, String>;
}
```

```rust
pub enum ToolTag {
    /// Intel-gathering, discovery, read-only inspection.
    Recon,
    /// File or state mutation.
    Edit,
    /// Tests, lints, checks, validation.
    Verify,
    /// External data retrieval (HTTP, MCP, web search, etc.).
    Fetch,
    /// Sub-agent or task creation.
    Spawn,
    /// Planning, design-doc, strategy, roadmap.
    Plan,
    /// Shell, cargo, runtime commands, and communication dispatch.
    Execute,
    /// Document / report / slide / spreadsheet generation and output-shaping.
    Format,
}
```

```rust
pub struct ToolDef {
    /// Validated tool name.
    pub name: ToolName,
    /// Short description sent to the LLM (token-budget friendly).
    pub description: String,
    /// Detailed description for extended-thinking mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extended_description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: InputSchema,
    /// Semantic category.
    pub category: ToolCategory,
    /// How reversible this tool's effects are.
    pub reversibility: Reversibility,
    /// Whether the tool activates automatically by domain without explicit config.
    pub auto_activate: bool,
    /// Tool groups this tool belongs to.  Used for role-based gating.
    #[serde(default)]
    pub groups: Vec<ToolGroupId>,
    /// Operational-intent tags for cross-category lookup.
    #[serde(default)]
    pub tags: Vec<ToolTag>,
}
```

```rust
pub struct InputSchema {
    /// Property definitions, insertion-ordered.
    pub properties: IndexMap<String, PropertyDef>,
    /// Names of required properties.
    pub required: Vec<String>,
}
```

```rust
impl InputSchema {
    pub fn to_json_schema (&self) -> serde_json::Value;
}
```

```rust
pub struct PropertyDef {
    /// The JSON Schema type.
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    /// Human-readable description.
    pub description: String,
    /// Allowed enum values, if constrained.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}
```

```rust
pub enum PropertyType {
    /// JSON string type.
    String,
    /// JSON number type (float or integer).
    Number,
    /// JSON integer type.
    Integer,
    /// JSON boolean type.
    Boolean,
    /// JSON array type.
    Array,
    /// JSON object type.
    Object,
}
```

```rust
pub enum Reversibility {
    /// Read-only operations with no side effects (ls, read, search).
    FullyReversible,
    /// Can be undone (write file with backup, git commit can revert).
    Reversible,
    /// Partial undo possible (delete file can restore from backup if exists).
    PartiallyReversible,
    /// Cannot be undone (exec with external side effects, API calls, messages).
    #[default]
    Irreversible,
}
```

```rust
impl Reversibility {
    pub fn supports_dry_run (self) -> bool;
}
```

```rust
pub enum ApprovalRequirement {
    /// No approval needed (read-only, fully reversible).
    None,
    /// Approval recommended but not enforced.
    Advisory,
    /// Approval required before execution.
    Required,
    /// Approval required with explicit confirmation prompt.
    Mandatory,
}
```

```rust
pub struct ToolCallMetadata {
    /// The tool's reversibility classification at call time.
    pub reversibility: Reversibility,
    /// The approval requirement that was applied.
    pub approval: ApprovalRequirement,
    /// Whether the call was a dry-run simulation.
    pub dry_run: bool,
}
```

```rust
pub enum ToolCategory {
    /// Filesystem operations.
    Workspace,
    /// Memory operations.
    Memory,
    /// Messaging and cross-agent communication.
    Communication,
    /// Planning and deliberation.
    Planning,
    /// System and configuration.
    System,
    /// Agent coordination and spawning.
    Agent,
    /// Web research and information retrieval.
    Research,
    /// External domain pack tools.
    Domain,
}
```

```rust
impl ToolCategory {
    pub fn icon (self) -> &'static str;
    pub fn display_name (self) -> &'static str;
    pub fn is_read_only (self) -> bool;
    pub fn is_destructive (self) -> bool;
}
```

```rust
pub struct ToolDiagnostics {
    /// Subprocess exit code, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Captured stderr output, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Sandbox policy violations that occurred during execution.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sandbox_violations: Vec<String>,
    /// Wall-clock execution duration in milliseconds.
    pub duration_ms: u64,
}
```

```rust
impl ToolDiagnostics {
    pub fn to_llm_text (&self) -> String;
}
```

```rust
pub enum ToolOutcome {
    /// All sub-operations succeeded.
    #[default]
    Success,
    /// Tool returned usable output, but some sub-operations failed or
    /// emitted warnings. Payload is boxed to keep `ToolOutcome`
    /// (and therefore `ToolResult`) small so that
    /// `Result<_, ToolResult>` helpers stay under the
    /// `clippy::result_large_err` threshold.
    PartialSuccess(Box<PartialSuccessInfo>),
    /// Tool failed; no usable output.
    Failure(Box<FailureInfo>),
}
```

```rust
pub struct PartialSuccessInfo {
    /// One reason per degraded sub-operation.
    pub reasons: Vec<String>,
}
```

```rust
pub struct FailureInfo {
    /// Single human-readable failure reason.
    pub reason: String,
}
```

```rust
impl ToolOutcome {
    pub fn partial (reasons: impl IntoIterator<Item = String>) -> Self;
    pub fn failure (reason: impl Into<String>) -> Self;
    pub fn is_error (&self) -> bool;
    pub fn is_success (&self) -> bool;
    pub fn is_partial (&self) -> bool;
    pub fn partial_reasons (&self) -> &[String];
    pub fn failure_reason (&self) -> &str;
}
```

```rust
pub struct ToolResult {
    /// Result content: text or rich content blocks.
    pub content: ToolResultContent,
    /// Whether this result represents an error.
    ///
    /// Retained for backward compatibility and wire-format stability
    /// (serialized LLM-facing envelopes and persisted sessions).
    /// New code should inspect [`ToolResult::outcome`] for the
    /// partial-success distinction (#3633); `is_error` stays
    /// synchronized with it via the constructors and remains a
    /// faithful binary collapse: `Success` or `PartialSuccess`
    /// → `false`, `Failure` → `true`.
    pub is_error: bool,
    /// Rich outcome classification. Defaults to `Success` when a
    /// legacy payload without this field is deserialized; the
    /// `is_error` flag is not consulted during deserialization
    /// because `#[serde(default)]` runs before field population.
    /// For legacy inputs, call [`ToolResult::normalize`] to reconcile.
    #[serde(default)]
    pub outcome: ToolOutcome,
    /// Optional diagnostic metadata from the execution environment.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diagnostics: Option<ToolDiagnostics>,
    /// HMAC-SHA256 receipt for hallucination-resistant attestation.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub receipt: Option<String>,
}
```

```rust
impl ToolResult {
    pub fn text (content: impl Into<String>) -> Self;
    pub fn error (content: impl Into<String>) -> Self;
    pub fn blocks (blocks: Vec<ToolResultBlock>) -> Self;
    pub fn partial_success (
        content: impl Into<String>,
        reasons: impl IntoIterator<Item = String>,
    ) -> Self;
    pub fn normalize (mut self) -> Self;
    pub fn with_diagnostics (mut self, diagnostics: ToolDiagnostics) -> Self;
    pub fn with_receipt (mut self, receipt: impl Into<String>) -> Self;
}
```

```rust
pub struct ToolInput {
    /// Which tool to invoke.
    pub name: ToolName,
    /// The `tool_use` block ID from the LLM response.
    pub tool_use_id: String,
    /// The arguments the LLM provided.
    pub arguments: serde_json::Value,
}
```

```rust
pub struct ToolStats {
    pub total_calls: u32,
    pub total_duration_ms: u64,
    pub error_count: u32,
    /// Count of calls that produced `ToolOutcome::PartialSuccess`
    /// (see #3633). Separate from `error_count` because partial
    /// successes deliver usable output even with degraded
    /// sub-operations.
    pub partial_count: u32,
    pub calls_by_tool: IndexMap<String, u32>,
}
```

```rust
impl ToolStats {
    pub fn record (&mut self, name: &str, duration_ms: u64, is_error: bool);
    pub fn record_outcome (&mut self, name: &str, duration_ms: u64, outcome: &ToolOutcome);
    pub fn top_tools (&self, n: usize) -> Vec<(&str, u32)>;
}
```

## `src/types/services.rs`

> Cross-nous message routing for tool executors.
```rust
pub trait CrossNousService : Send + Sync {
    fn send (
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;
    fn ask (
        &self,
        from: &str,
        to: &str,
        session_key: &str,
        content: &str,
        timeout_secs: u64,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + '_>>;
}
```

> Outbound message delivery (Signal, etc.) for tool executors.
```rust
pub trait MessageService : Send + Sync {
    fn send_message (
        &self,
        to: &str,
        text: &str,
        from_nous: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;
}
```

> Planning project management for tool executors.
```rust
pub trait PlanningService : Send + Sync {
    fn create_project (
        &self,
        name: &str,
        description: &str,
        scope: Option<&str>,
        mode: &str,
        appetite_minutes: Option<u32>,
        owner: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn load_project (
        &self,
        project_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn transition_project (
        &self,
        project_id: &str,
        transition: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn add_phase (
        &self,
        project_id: &str,
        name: &str,
        goal: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn add_plan (
        &self,
        project_id: &str,
        phase_id: &str,
        plan: PlanningPlanInput<'_>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn complete_plan (
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        achievement: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn fail_plan (
        &self,
        project_id: &str,
        phase_id: &str,
        plan_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn list_projects (
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
    fn verify_criteria (
        &self,
        project_id: &str,
        phase_id: &str,
        criteria_json: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PlanningAdapterError>> + Send + '_>>;
}
```

> Input for creating an executable plan inside a phase.
```rust
pub struct PlanningPlanInput<'a> {
    /// Short title for the executable plan.
    pub title: &'a str,
    /// Concrete work description.
    pub description: &'a str,
    /// Execution wave; plans in the same wave may run in parallel.
    pub wave: u32,
    /// Plan IDs that must complete before this plan can run.
    pub depends_on: &'a [String],
    /// Optional maximum iterations before the plan is stuck.
    pub max_iterations: Option<u32>,
}
```

```rust
pub struct MemoryResult {
    pub id: String,
    pub content: String,
    pub score: f64,
    pub source_type: String,
}
```

```rust
pub struct FactSummary {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub recorded_at: String,
    pub is_forgotten: bool,
    pub forgotten_at: Option<String>,
    pub forget_reason: Option<String>,
}
```

> Abstracts knowledge store operations for memory tools.
> 
> Implemented by an adapter in the binary crate wrapping `KnowledgeStore` + `EmbeddingProvider`.
```rust
pub trait KnowledgeSearchService : Send + Sync {
    fn search (
        &self,
        query: &str,
        nous_id: &str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryResult>, KnowledgeAdapterError>> + Send + '_>>;
    fn correct_fact (
        &self,
        fact_id: &str,
        new_content: &str,
        nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, KnowledgeAdapterError>> + Send + '_>>;
    fn retract_fact (
        &self,
        fact_id: &str,
        reason: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), KnowledgeAdapterError>> + Send + '_>>;
    fn audit_facts (
        &self,
        nous_id: Option<&str>,
        since: Option<&str>,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FactSummary>, KnowledgeAdapterError>> + Send + '_>>;
    fn forget_fact (
        &self,
        fact_id: &str,
        reason: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>;
    fn unforget_fact (
        &self,
        fact_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<FactSummary, KnowledgeAdapterError>> + Send + '_>>;
    fn datalog_query (
        &self,
        query: &str,
        params: Option<serde_json::Value>,
        timeout_secs: Option<f64>,
        row_limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<DatalogResult, KnowledgeAdapterError>> + Send + '_>>;
    fn find_skill_by_name (
        &self,
        nous_id: &str,
        skill_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, KnowledgeAdapterError>> + Send + '_>>;
}
```

```rust
pub struct DatalogResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}
```

> Persistent session notes storage.
```rust
pub trait NoteStore : Send + Sync {
    fn add_note (
        &self,
        session_id: &str,
        nous_id: &str,
        category: &str,
        content: &str,
    ) -> std::result::Result<i64, crate::error::StoreError>;
    fn get_notes (
        &self,
        session_id: &str,
    ) -> std::result::Result<Vec<NoteEntry>, crate::error::StoreError>;
    fn delete_note (&self, note_id: i64) -> std::result::Result<bool, crate::error::StoreError>;
}
```

> Shared blackboard state with TTL.
```rust
pub trait BlackboardStore : Send + Sync {
    fn write (
        &self,
        key: &str,
        value: &str,
        author: &str,
        ttl_seconds: i64,
    ) -> std::result::Result<(), crate::error::StoreError>;
    fn read (
        &self,
        key: &str,
    ) -> std::result::Result<Option<BlackboardEntry>, crate::error::StoreError>;
    fn list (&self) -> std::result::Result<Vec<BlackboardEntry>, crate::error::StoreError>;
    fn delete (
        &self,
        key: &str,
        author: &str,
    ) -> std::result::Result<bool, crate::error::StoreError>;
}
```

```rust
pub struct NoteEntry {
    pub id: i64,
    pub category: String,
    pub content: String,
    pub created_at: String,
}
```

```rust
pub struct BlackboardEntry {
    pub key: String, // kanon:ignore RUST/plain-string-secret
    pub value: String,
    pub author_nous_id: String,
    pub ttl_seconds: i64,
    pub created_at: String,
    pub expires_at: Option<String>,
}
```

```rust
pub struct SpawnRequest {
    /// Role identifier (coder, reviewer, researcher, explorer, runner).
    pub role: String,
    /// Task prompt sent as the single turn.
    pub task: String,
    /// Model override (None = role-based default).
    pub model: Option<String>,
    /// Tool name allowlist (None = role-based defaults).
    pub allowed_tools: Option<Vec<String>>,
    /// Maximum seconds before the sub-agent is killed.
    pub timeout_secs: u64,
}
```

```rust
pub struct SpawnResult {
    /// The sub-agent's text response.
    pub content: String,
    /// Whether the sub-agent encountered an error.
    pub is_error: bool,
    /// Input tokens consumed.
    pub input_tokens: u64,
    /// Output tokens produced.
    pub output_tokens: u64,
}
```

> Working checkpoint storage for agent-curated session memory.
> 
> Agents call `update_working_checkpoint` to persist structured key-info
> that survives context compaction and is reinjected on subsequent turns.
```rust
pub trait WorkingCheckpointStore : Send + Sync {
    fn write_checkpoint (
        &self,
        session_id: &str,
        turn_number: u64,
        content: &str,
    ) -> std::result::Result<(), crate::error::StoreError>;
    fn read_latest (
        &self,
        session_id: &str,
    ) -> std::result::Result<Option<WorkingCheckpoint>, crate::error::StoreError>;
    fn read_recent (
        &self,
        session_id: &str,
        limit: usize,
    ) -> std::result::Result<Vec<WorkingCheckpoint>, crate::error::StoreError>;
}
```

```rust
pub struct WorkingCheckpoint {
    /// Session identifier.
    pub session_id: String,
    /// Turn number when the checkpoint was written.
    pub turn_number: u64,
    /// Structured key-info content.
    pub content: String,
    /// ISO-8601 timestamp of the write.
    pub created_at: String,
}
```

> Ephemeral sub-agent spawning for tool executors.
```rust
pub trait SpawnService : Send + Sync {
    fn spawn_and_run (
        &self,
        request: SpawnRequest,
        parent_nous_id: &str,
    ) -> Pin<Box<dyn Future<Output = Result<SpawnResult, String>> + Send + '_>>;
}
```
