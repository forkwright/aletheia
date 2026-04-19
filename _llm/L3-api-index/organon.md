# L3 API Index: organon

Crate path: `crates/organon`

Public API signatures extracted from source. Doc comments shown above each signature.
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

> Register all 9 energeia tools with real implementations.
> 
> When `services` is `Some`, tools that need the orchestrator or store call
> through to the real energeia subsystem. When `None`, those tools return a
> structured error indicating the missing dependency — they do not panic.
> 
> Tools that are pure computation (schedion, prographe, diorthosis,
> dokimasia, epitropos) work regardless of whether services are provided.
> 
> # Errors
> 
> Returns an error if any tool name collides with an already-registered tool.
```rust
pub fn register (
    registry: &mut ToolRegistry,
    services: Option<Arc<EnergeiaServices>>,
) -> Result<()>
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

## `src/metrics.rs`

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
}
```

```rust
impl ToolRegistry {
    pub fn new () -> Self;
    pub fn register (&mut self, def: ToolDef, executor: Box<dyn ToolExecutor>) -> Result<()>;
    pub fn get_def (&self, name: &ToolName) -> Option<&ToolDef>;
    pub async fn execute (&self, input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult>;
    pub fn definitions (&self) -> Vec<&ToolDef>;
    pub fn definitions_for_category (&self, category: ToolCategory) -> Vec<&ToolDef>;
    pub fn to_hermeneus_tools (&self) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn to_hermeneus_tools_filtered (
        // kanon:ignore RUST/pub-visibility
        &self,
        active: &HashSet<ToolName>,
    ) -> Vec<hermeneus::types::ToolDefinition>;
    pub fn reversibility (&self, name: &ToolName) -> Option<Reversibility>;
    pub fn approval_requirement (&self, name: &ToolName) -> Option<ApprovalRequirement>;
    pub fn call_metadata (&self, name: &ToolName, dry_run: bool) -> Option<ToolCallMetadata>;
    pub fn lazy_tool_catalog (&self) -> Vec<(ToolName, String)>;
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
    /// (dotfiles, project repos, etc.) even in permissive mode: closes #1823.
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
    /// Closes #1984.
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
    pub http_client: reqwest::Client,
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
pub struct ToolResult {
    /// Result content: text or rich content blocks.
    pub content: ToolResultContent,
    /// Whether this result represents an error.
    pub is_error: bool,
    /// Optional diagnostic metadata from the execution environment.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub diagnostics: Option<ToolDiagnostics>,
}
```

```rust
impl ToolResult {
    pub fn text (content: impl Into<String>) -> Self;
    pub fn error (content: impl Into<String>) -> Self;
    pub fn blocks (blocks: Vec<ToolResultBlock>) -> Self;
    pub fn with_diagnostics (mut self, diagnostics: ToolDiagnostics) -> Self;
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
    pub calls_by_tool: IndexMap<String, u32>,
}
```

```rust
impl ToolStats {
    pub fn record (&mut self, name: &str, duration_ms: u64, is_error: bool);
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
