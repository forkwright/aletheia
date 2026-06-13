# L3 API Index: thesauros

Crate path: `crates/thesauros`

Public API signatures extracted from source. Each signature is preceded by its doc comment.
For implementation context, read the source directly (`L4`).

## `src/error.rs`

```rust
pub enum Error {
    /// Pack directory does not exist.
    #[snafu(display("pack not found: {}", path.display()))]
    PackNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Manifest file (pack.toml) not found in pack directory.
    #[snafu(display("manifest not found: {}", path.display()))]
    ManifestNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to read a file.
    #[snafu(display("failed to read {}", path.display()))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to parse TOML manifest.
    #[snafu(display("failed to parse manifest at {}: {reason}", path.display()))]
    ParseManifest {
        path: PathBuf,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A context file referenced by the manifest was not found.
    #[snafu(display("context file not found: {}", path.display()))]
    ContextFileNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Context file path escapes the pack root directory.
    #[snafu(display("context file path escapes pack root: {}", path.display()))]
    ContextFileEscape {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool command script not found at declared path.
    #[snafu(display("tool command not found: {}", path.display()))]
    ToolCommandNotFound {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool command path resolves outside the pack root.
    #[snafu(display("tool command escapes pack root: {}", path.display()))]
    ToolCommandEscape {
        path: PathBuf,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Unknown property type in a tool's input schema.
    #[snafu(display("unknown property type '{type_name}' in tool '{tool_name}'"))]
    UnknownPropertyType {
        type_name: String,
        tool_name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to register a pack tool in the registry.
    #[snafu(display("failed to register tool '{tool_name}' from pack '{pack_name}': {reason}"))]
    ToolRegistration {
        tool_name: String,
        pack_name: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pack name fails validation (must be 1--64 alphanumeric/hyphen characters).
    #[snafu(display(
        "invalid pack name '{name}': must be 1-64 characters, alphanumeric and hyphens only"
    ))]
    InvalidPackName {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Pack version is an empty string.
    #[snafu(display("pack '{pack}' has an empty version string"))]
    InvalidPackVersion {
        pack: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

## `src/loader.rs`

```rust
pub struct PackSection {
    /// Section name (derived from filename, e.g. `BUSINESS_LOGIC.md`).
    pub name: String,
    /// The text content.
    pub content: String,
    /// Bootstrap priority level.
    pub priority: Priority,
    /// Whether this section can be truncated under budget pressure.
    pub truncatable: bool,
    /// Optional agent filter. Empty = available to all agents.
    pub agents: Vec<String>,
    /// Which pack this section came from.
    pub pack_name: String,
}
```

```rust
pub struct LoadedPack {
    /// The pack manifest.
    pub manifest: PackManifest,
    /// Resolved context sections with file contents read.
    pub sections: Vec<PackSection>,
    /// Absolute path to the pack root.
    pub root: PathBuf,
}
```

```rust
impl LoadedPack {
    pub fn sections_for_agent_or_domains (
        &self,
        agent_id: &str,
        domains: &[String],
    ) -> Vec<&PackSection>;
    pub fn domains_for_agent (&self, agent_id: &str) -> Vec<String>;
    pub fn model_for_agent (&self, agent_id: &str) -> Option<String>;
    pub fn agency_for_agent (&self, agent_id: &str) -> Option<String>;
    pub fn system_prompt_additions_for_agent (&self, agent_id: &str) -> Vec<String>;
}
```

> Load all configured domain packs.
> 
> Reads manifests from each path, resolves context files, and returns loaded packs.
> Invalid or missing packs emit warnings and are skipped (graceful degradation).
> 
> # Blocking I/O
> 
> This function performs synchronous file I/O and is intended to be called once
> at startup, before the async runtime begins serving requests. If called from
> within an async context during normal operation, wrap in
> `tokio::task::spawn_blocking`.
```rust
pub fn load_packs (paths: &[PathBuf]) -> Vec<LoadedPack>
```

## `src/manifest.rs`

```rust
pub struct PackManifest {
    /// Pack name (e.g. "acme-analytics").
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Context files to inject into bootstrap.
    #[serde(default)]
    pub context: Vec<ContextEntry>,
    /// Tool definitions provided by this pack.
    #[serde(default)]
    pub tools: Vec<PackToolDef>,
    /// Per-agent config overlays.
    #[serde(default)]
    pub overlays: std::collections::HashMap<String, AgentOverlay>,
}
```

```rust
pub struct ContextEntry {
    /// Path relative to pack root.
    pub path: String,
    /// Bootstrap priority level.
    #[serde(default = "default_priority")]
    pub priority: Priority,
    /// Optional agent filter. Empty = all agents.
    #[serde(default)]
    pub agents: Vec<String>,
    /// Whether this section can be truncated under budget pressure.
    #[serde(default)]
    pub truncatable: bool,
}
```

```rust
pub enum Priority {
    /// Section is always included and cannot be truncated.
    Required,
    /// Section is included unless context is critically full.
    Important,
    /// Section may be truncated to save context.
    Flexible,
    /// Section is omitted first when trimming context.
    Optional,
}
```

```rust
pub struct AgentOverlay {
    /// Domain tags to merge into the agent's config.
    #[serde(default)]
    pub domains: Vec<String>,

    /// Optional override for primary model. None means use the agent's configured model.
    #[serde(default)]
    pub model: Option<String>,

    /// Optional override for agency level (unrestricted, standard, restricted).
    /// None means use the agent's default.
    #[serde(default)]
    pub agency: Option<String>,

    /// Per-agent system-prompt additions appended at the workspace-pack tier.
    #[serde(default)]
    pub system_prompt_additions: Vec<String>,
}
```

```rust
pub struct PackToolDef {
    /// Tool name (must be a valid `ToolName`).
    pub name: String,
    /// Short description sent to the LLM.
    pub description: String,
    /// Path to executable script, relative to pack root.
    pub command: String,
    /// Execution timeout in milliseconds.
    #[serde(default = "default_tool_timeout")]
    pub timeout: u64,
    /// Input parameter schema.
    #[serde(default)]
    pub input_schema: Option<PackInputSchema>,
    /// Capability groups for tool gating. Defaults to `["command"]`.
    #[serde(default)]
    pub groups: Vec<String>,
    /// Operational intent tags. Defaults to `["execute"]`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Reversibility metadata. Defaults to `irreversible`.
    #[serde(default)]
    pub reversibility: Option<String>,
}
```

```rust
pub struct PackInputSchema {
    /// Property definitions, insertion-ordered.
    #[serde(default)]
    pub properties: IndexMap<String, PackPropertyDef>,
    /// Names of required properties.
    #[serde(default)]
    pub required: Vec<String>,
}
```

```rust
pub struct PackPropertyDef {
    /// JSON Schema type name ("string", "number", "integer", "boolean", "array", "object").
    #[serde(rename = "type")]
    pub property_type: String,
    /// Human-readable description.
    pub description: String,
    /// Allowed enum values, if constrained.
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Default value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}
```

## `src/tools/mod.rs`

> Register all tools from loaded packs into the tool registry.
> 
> Validates each tool's command path and schema, then registers it.
> Invalid tools are skipped with warnings; errors are collected and returned.
```rust
pub fn register_pack_tools (packs: &[LoadedPack], registry: &mut ToolRegistry) -> Vec<error::Error>
```

> Register all tools from loaded packs with the supplied subprocess sandbox.
> 
> Runtime callers pass the same sandbox config used by built-in tools so pack
> shell tools inherit the deployment's process, filesystem, and egress policy.
```rust
pub fn register_pack_tools_with_sandbox (
    packs: &[LoadedPack],
    registry: &mut ToolRegistry,
    sandbox: organon::sandbox::SandboxConfig,
) -> Vec<error::Error>
```
