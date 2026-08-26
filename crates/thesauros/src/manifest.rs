//! Pack manifest parsing and validation.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, ensure};

use crate::error::{self, Result};

/// Manifest filename expected in every pack root.
const MANIFEST_FILENAME: &str = "pack.toml";

/// A parsed and validated domain pack manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// A context file entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Bootstrap priority levels matching `SectionPriority` in nous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
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

fn default_priority() -> Priority {
    Priority::Important
}

/// Per-agent configuration overlay from a pack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// Default total byte cap for one pack's system-prompt additions per agent
/// when the operator opts into prompt additions.
pub const DEFAULT_MAX_PROMPT_ADDITIONS_BYTES: usize = 4096;

/// Operator policy for high-impact pack overlay powers (#5220).
///
/// `model`, `agency`, and `system_prompt_additions` change model choice,
/// agent autonomy, and durable prompt text — a domain pack must not raise
/// tool iterations or inject non-truncatable prompt text silently. The
/// default is restrictive: those powers are stripped at load (recorded in
/// pack health) until the operator opts in via `[packOverlays]` in
/// `aletheia.toml`. Domain tags are low-impact routing hints and always
/// apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPolicy {
    /// Permit packs to override an agent's primary model.
    pub allow_model_overrides: bool,
    /// Permit packs to override an agent's agency level.
    pub allow_agency_overrides: bool,
    /// Permit packs to inject durable system-prompt additions.
    pub allow_prompt_additions: bool,
    /// Total byte cap for one pack's system-prompt additions per agent.
    /// Additions past the cap are dropped (never truncated mid-string).
    pub max_prompt_additions_bytes: usize,
}

impl Default for OverlayPolicy {
    fn default() -> Self {
        Self {
            allow_model_overrides: false,
            allow_agency_overrides: false,
            allow_prompt_additions: false,
            max_prompt_additions_bytes: DEFAULT_MAX_PROMPT_ADDITIONS_BYTES,
        }
    }
}

impl OverlayPolicy {
    /// Permit every overlay power. For tests and for operators who want the
    /// pre-#5220 behavior; production runtimes should build the policy from
    /// the operator's config instead.
    #[must_use]
    pub fn permit_all() -> Self {
        Self {
            allow_model_overrides: true,
            allow_agency_overrides: true,
            allow_prompt_additions: true,
            max_prompt_additions_bytes: DEFAULT_MAX_PROMPT_ADDITIONS_BYTES,
        }
    }
}

/// A tool definition declared in a pack manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Reserved environment-authority declarations (#5214).
    ///
    /// Non-empty declarations are rejected until an operator-owned,
    /// per-pack/per-tool grant can be intersected with this request. A pack
    /// manifest is not authority to read arbitrary daemon environment values.
    #[serde(default)]
    pub env: Vec<String>,
    /// Reserved filesystem-write declarations (#5214).
    ///
    /// Non-empty declarations are rejected until they can be intersected
    /// with operator policy. A manifest alone cannot widen write authority.
    #[serde(default)]
    pub write_paths: Vec<String>,
    /// Declared network egress intent: `"none"` denies outbound network for
    /// this tool (tightening the deployment sandbox policy when it is
    /// enabled). Absent or `"inherit"` leaves the deployment policy
    /// unchanged. A pack can only narrow egress, never widen it.
    #[serde(default)]
    pub egress: Option<String>,
    /// Host platforms this tool supports: any of `linux`, `macos`, `unix`
    /// (#5215).
    ///
    /// Empty means `["unix"]`: a pack tool is a shebang-executed script,
    /// which needs a Unix exec environment unless the author declares
    /// otherwise. A tool whose list does not cover the current host is
    /// skipped at registration and the pack is marked degraded.
    #[serde(default)]
    pub platforms: Vec<String>,
}

fn default_tool_timeout() -> u64 {
    30_000
}

/// Input schema for a pack tool, matching JSON Schema structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInputSchema {
    /// Property definitions, insertion-ordered.
    #[serde(default)]
    pub properties: IndexMap<String, PackPropertyDef>,
    /// Names of required properties.
    #[serde(default)]
    pub required: Vec<String>,
}

/// A single property in a pack tool's input schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Load and parse a pack manifest from a directory.
///
/// Reads `{pack_root}/pack.toml`, validates structure, and returns the parsed manifest.
///
/// # Errors
///
/// - [`error::Error::PackNotFound`] if `pack_root` does not exist
/// - [`error::Error::ManifestNotFound`] if `pack.toml` is missing
/// - [`error::Error::ReadFile`] if the file cannot be read
/// - [`error::Error::ParseManifest`] if TOML parsing fails
pub(crate) fn load_manifest(pack_root: &Path) -> Result<PackManifest> {
    ensure!(
        pack_root.is_dir(),
        error::PackNotFoundSnafu { path: pack_root }
    );

    let manifest_path = pack_root.join(MANIFEST_FILENAME);
    ensure!(
        manifest_path.is_file(),
        error::ManifestNotFoundSnafu {
            path: &manifest_path
        }
    );

    let contents = std::fs::read_to_string(&manifest_path).context(error::ReadFileSnafu {
        path: manifest_path.clone(),
    })?;

    let manifest: PackManifest =
        toml::from_str(&contents).map_err(|e| error::Error::ParseManifest {
            path: manifest_path,
            reason: e.to_string(),
            location: snafu::location!(),
        })?;

    validate_manifest(&manifest)?;

    Ok(manifest)
}

/// Validate the manifest contract: pack identity, tool declarations, and
/// overlay powers.
///
/// All problems are collected into one [`error::Error::InvalidManifest`]
/// diagnostic instead of failing on the first, so a pack author sees the
/// full list in a single load attempt. An invalid manifest fails the pack
/// load; the pack never activates partially.
///
/// Tool groups, tags, and reversibility are additionally validated against
/// organon's types at registration time (`tools::prepare_tool`), where their
/// parsed values are needed; failures there are recorded in pack health.
fn validate_manifest(manifest: &PackManifest) -> Result<()> {
    let mut issues = Vec::new();

    if !is_valid_pack_name(&manifest.name) {
        issues.push(format!(
            "invalid pack name '{}': must be 1-64 characters, alphanumeric and hyphens only",
            manifest.name
        ));
    }

    if manifest.version.is_empty() {
        issues.push("pack version is an empty string".to_owned());
    }

    for tool in &manifest.tools {
        validate_tool_contract(tool, &mut issues);
    }

    for (agent, overlay) in &manifest.overlays {
        validate_overlay(agent, overlay, &mut issues);
    }

    if issues.is_empty() {
        return Ok(());
    }
    Err(error::Error::InvalidManifest {
        pack: manifest.name.clone(),
        issues,
        location: snafu::location!(),
    })
}

/// Validate one tool declaration's load-time contract: name shape, timeout,
/// and command-path syntax. Filesystem checks (exists, executable, in-pack
/// after canonicalization) stay at registration, where a failure degrades
/// the pack instead of rejecting the whole manifest.
fn validate_tool_contract(tool: &PackToolDef, issues: &mut Vec<String>) {
    if let Err(e) = koina::id::ToolName::new(tool.name.as_str()) {
        issues.push(format!("tool '{}': invalid tool name: {e}", tool.name));
    }
    if tool.timeout == 0 {
        issues.push(format!(
            "tool '{}' has an invalid timeout of 0ms: must be non-zero",
            tool.name
        ));
    }
    if !is_relative_in_pack_path(Path::new(&tool.command)) {
        issues.push(format!(
            "tool '{}' command '{}' must be a relative path inside the pack root",
            tool.name, tool.command
        ));
    }
    if !tool.env.is_empty() {
        issues.push(format!(
            "tool '{}' declares env authority, but pack env grants are reserved until an \
             operator-owned per-tool policy exists",
            tool.name
        ));
    }
    if !tool.write_paths.is_empty() {
        issues.push(format!(
            "tool '{}' declares write_paths authority, but pack write grants are reserved until \
             they can be intersected with operator policy",
            tool.name
        ));
    }
    if let Some(egress) = tool.egress.as_deref()
        && !matches!(egress, "none" | "inherit")
    {
        issues.push(format!(
            "tool '{}' egress '{egress}' is unknown (expected \"none\" or \"inherit\")",
            tool.name
        ));
    }
    for platform in &tool.platforms {
        if !matches!(platform.as_str(), "linux" | "macos" | "unix") {
            issues.push(format!(
                "tool '{}' platform '{platform}' is unknown \
                 (expected \"linux\", \"macos\", or \"unix\")",
                tool.name
            ));
        }
    }
}

/// Validate one overlay's high-impact fields: `agency` must name a known
/// level (the runtime maps it to iteration limits), and `model` /
/// `system_prompt_additions` must not be blank placeholders that would apply
/// as empty overrides, or contain `{{file:...}}` interpolation. File expansion
/// happens after the pack byte cap and would otherwise amplify a tiny declared
/// addition into unbounded non-truncatable prompt authority.
fn validate_overlay(agent: &str, overlay: &AgentOverlay, issues: &mut Vec<String>) {
    if let Some(agency) = overlay.agency.as_deref()
        && !matches!(agency, "unrestricted" | "standard" | "restricted")
    {
        issues.push(format!(
            "overlay for agent '{agent}': unknown agency '{agency}' \
             (expected \"unrestricted\", \"standard\", or \"restricted\")"
        ));
    }
    if let Some(model) = overlay.model.as_deref()
        && model.trim().is_empty()
    {
        issues.push(format!(
            "overlay for agent '{agent}': model override is blank"
        ));
    }
    for addition in &overlay.system_prompt_additions {
        if addition.trim().is_empty() {
            issues.push(format!(
                "overlay for agent '{agent}': system-prompt addition is blank"
            ));
        }
        if addition.contains("{{file:") {
            issues.push(format!(
                "overlay for agent '{agent}': system-prompt additions must not contain \
                 {{{{file:...}}}} interpolation; declare bounded pack context instead"
            ));
        }
    }
}

/// Returns `true` when `declared` is a relative path made only of normal and
/// `.` components — i.e. it cannot escape a pack root when joined to it.
///
/// Syntactic check shared by manifest validation (no filesystem access) and
/// registration-time command validation.
pub(crate) fn is_relative_in_pack_path(declared: &Path) -> bool {
    !declared.is_absolute()
        && declared.components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// Returns `true` if the name is 1--64 ASCII alphanumeric/hyphen characters.
fn is_valid_pack_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Resolve a context entry path relative to the pack root.
///
/// Returns the canonical absolute path, or an error if the file does not
/// exist or if the resolved path escapes the pack root directory.
pub(crate) fn resolve_context_path(pack_root: &Path, entry: &ContextEntry) -> Result<PathBuf> {
    let resolved = pack_root.join(&entry.path);
    ensure!(
        resolved.is_file(),
        error::ContextFileNotFoundSnafu { path: &resolved }
    );

    // WHY: canonicalize resolves all symlinks and parent-dir components, then
    // verify the result is still under the pack root to prevent path traversal
    let canonical = resolved.canonicalize().context(error::ReadFileSnafu {
        path: resolved.clone(),
    })?;
    let canonical_root = pack_root.canonicalize().context(error::ReadFileSnafu {
        path: pack_root.to_path_buf(),
    })?;
    ensure!(
        canonical.starts_with(&canonical_root),
        error::ContextFileEscapeSnafu { path: &resolved }
    );

    Ok(canonical)
}

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn setup_pack(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "thesauros pack loader reads binary assets from disk; synchronous I/O is inherent to asset loading"
            )]
            fs::write(&path, content).unwrap();
        }
        dir
    }

    fn minimal_manifest() -> &'static str {
        "name = \"test-pack\"\nversion = \"1.0\"\n"
    }

    #[test]
    fn load_minimal_manifest() {
        let dir = setup_pack(&[("pack.toml", minimal_manifest())]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name, "test-pack");
        assert_eq!(manifest.version, "1.0");
        assert!(manifest.context.is_empty());
        assert!(manifest.overlays.is_empty());
    }

    #[test]
    fn load_full_manifest() {
        let toml = r#"
name = "acme-analytics"
version = "1.0"
description = "Acme Corp analytics domain pack"

[[context]]
path = "context/BUSINESS_LOGIC.md"
priority = "important"
agents = ["analyst"]
truncatable = false

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[[context]]
path = "context/SQL_PATTERNS.md"
priority = "important"

[overlays.analyst]
domains = ["healthcare", "analytics", "sql"]
"#;
        let dir = setup_pack(&[
            ("pack.toml", toml),
            ("context/BUSINESS_LOGIC.md", "business logic"),
            ("context/GLOSSARY.md", "glossary"),
            ("context/SQL_PATTERNS.md", "patterns"),
        ]);

        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name, "acme-analytics");
        assert_eq!(manifest.context.len(), 3);
        assert_eq!(manifest.context[0].priority, Priority::Important);
        assert_eq!(manifest.context[0].agents, vec!["analyst"]);
        assert!(!manifest.context[0].truncatable);
        assert_eq!(manifest.context[1].priority, Priority::Flexible);
        assert!(manifest.context[1].truncatable);
        assert!(manifest.context[2].agents.is_empty());

        let analyst = manifest.overlays.get("analyst").unwrap();
        assert_eq!(analyst.domains, vec!["healthcare", "analytics", "sql"]);
    }

    #[test]
    fn load_missing_pack_dir() {
        let err = load_manifest(Path::new("/nonexistent/pack")).unwrap_err();
        assert!(matches!(err, error::Error::PackNotFound { .. }));
    }

    #[test]
    fn load_missing_manifest_file() {
        let dir = TempDir::new().unwrap();
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(matches!(err, error::Error::ManifestNotFound { .. }));
    }

    #[test]
    fn load_invalid_toml() {
        let dir = setup_pack(&[("pack.toml", "{{{{invalid toml")]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(matches!(err, error::Error::ParseManifest { .. }));
    }

    #[test]
    fn resolve_context_path_found() {
        let dir = setup_pack(&[
            ("pack.toml", minimal_manifest()),
            ("context/LOGIC.md", "content"),
        ]);
        let entry = ContextEntry {
            path: "context/LOGIC.md".to_owned(),
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };
        let resolved = resolve_context_path(dir.path(), &entry).unwrap();
        assert!(resolved.ends_with("context/LOGIC.md"));
    }

    #[test]
    fn resolve_context_path_missing() {
        let dir = setup_pack(&[("pack.toml", minimal_manifest())]);
        let entry = ContextEntry {
            path: "context/MISSING.md".to_owned(),
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };
        let err = resolve_context_path(dir.path(), &entry).unwrap_err();
        assert!(matches!(err, error::Error::ContextFileNotFound { .. }));
    }

    #[test]
    fn resolve_context_path_blocks_parent_dir_traversal() {
        let outer = TempDir::new().unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "thesauros pack loader reads binary assets from disk; synchronous I/O is inherent to asset loading"
        )]
        fs::write(outer.path().join("secret.md"), "secret content").unwrap();

        let pack = TempDir::new().unwrap();

        let traversal = format!(
            "../{}/secret.md",
            outer.path().file_name().unwrap().to_string_lossy()
        );

        let entry = ContextEntry {
            path: traversal,
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };
        let err = resolve_context_path(pack.path(), &entry).unwrap_err();
        assert!(
            matches!(
                err,
                error::Error::ContextFileEscape { .. } | error::Error::ContextFileNotFound { .. }
            ),
            "traversal path must be rejected, got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_context_path_escape_does_not_leak_canonical_target() {
        use std::os::unix::fs::symlink;

        let outer = TempDir::new().unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "thesauros pack loader reads binary assets from disk; synchronous I/O is inherent to asset loading"
        )]
        fs::write(outer.path().join("secret.md"), "secret content").unwrap();

        let pack = TempDir::new().unwrap();
        let link = pack.path().join("escape.md");
        symlink(outer.path().join("secret.md"), &link).unwrap();

        let entry = ContextEntry {
            path: "escape.md".to_owned(),
            priority: Priority::Important,
            agents: vec![],
            truncatable: false,
        };
        let err = resolve_context_path(pack.path(), &entry).unwrap_err();
        let msg = err.to_string();
        assert!(
            matches!(err, error::Error::ContextFileEscape { .. }),
            "symlink escape must be rejected, got: {err}"
        );
        assert!(
            msg.contains("escape.md"),
            "error should identify the pack entry path, got: {msg}"
        );
        assert!(
            !msg.contains("secret.md"),
            "error must not leak resolved symlink target, got: {msg}"
        );
    }

    #[test]
    fn priority_default_is_important() {
        let toml = "name = \"test\"\nversion = \"1.0\"\n\n[[context]]\npath = \"file.md\"\n";
        let dir = setup_pack(&[("pack.toml", toml), ("file.md", "content")]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.context[0].priority, Priority::Important);
    }

    #[test]
    fn serde_roundtrip() {
        let manifest = PackManifest {
            name: "test".to_owned(),
            version: "1.0".to_owned(),
            description: Some("a test pack".to_owned()),
            context: vec![ContextEntry {
                path: "ctx/FILE.md".to_owned(),
                priority: Priority::Flexible,
                agents: vec!["analyst".to_owned()],
                truncatable: true,
            }],
            tools: vec![],
            overlays: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: PackManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.context[0].priority, Priority::Flexible);
    }

    #[test]
    fn load_manifest_with_tools() {
        let toml = r#"
name = "tool-pack"
version = "1.0"

[[tools]]
name = "query_redshift"
description = "Execute read-only SQL against Redshift"
command = "tools/query_redshift.sh"
timeout = 60000
groups = ["read"]
tags = ["recon", "fetch"]
reversibility = "fully_reversible"

[tools.input_schema]
required = ["sql"]

[tools.input_schema.properties.sql]
type = "string"
description = "SQL query to execute"

[[tools]]
name = "schema_lookup"
description = "Look up table schema"
command = "tools/schema_lookup.py"

[tools.input_schema]
required = ["table"]

[tools.input_schema.properties.table]
type = "string"
description = "Table name"
"#;
        let dir = setup_pack(&[
            ("pack.toml", toml),
            ("tools/query_redshift.sh", "#!/bin/sh"),
            ("tools/schema_lookup.py", "#!/usr/bin/env python3"),
        ]);

        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.tools.len(), 2);
        assert_eq!(manifest.tools[0].name, "query_redshift");
        assert_eq!(manifest.tools[0].timeout, 60_000);
        assert_eq!(manifest.tools[0].groups, vec!["read"]);
        assert_eq!(manifest.tools[0].tags, vec!["recon", "fetch"]);
        assert_eq!(
            manifest.tools[0].reversibility.as_deref(),
            Some("fully_reversible")
        );
        assert!(manifest.tools[0].input_schema.is_some());
        let schema = manifest.tools[0].input_schema.as_ref().unwrap();
        assert_eq!(schema.required, vec!["sql"]);
        assert_eq!(schema.properties["sql"].property_type, "string");

        assert_eq!(manifest.tools[1].timeout, 30_000);
    }

    #[test]
    fn manifest_without_tools_backward_compat() {
        let dir = setup_pack(&[("pack.toml", minimal_manifest())]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn rejects_zero_tool_timeout() {
        let toml = r#"
name = "tool-pack"
version = "1.0"

[[tools]]
name = "zero_timeout_tool"
description = "A tool with a zero timeout"
command = "tools/zero.sh"
timeout = 0
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
        assert!(err.to_string().contains("invalid timeout"), "{err}");
    }

    #[test]
    fn pack_tool_def_serde_roundtrip() {
        let tool = PackToolDef {
            name: "test_tool".to_owned(),
            description: "A test tool".to_owned(),
            command: "tools/test.sh".to_owned(),
            timeout: 45_000,
            input_schema: Some(PackInputSchema {
                properties: IndexMap::from([(
                    "query".to_owned(),
                    PackPropertyDef {
                        property_type: "string".to_owned(),
                        description: "Search query".to_owned(),
                        enum_values: None,
                        default: None,
                    },
                )]),
                required: vec!["query".to_owned()],
            }),
            groups: vec!["read".to_owned()],
            tags: vec!["recon".to_owned()],
            reversibility: Some("fully_reversible".to_owned()),
            env: Vec::new(),
            write_paths: Vec::new(),
            egress: None,
            platforms: Vec::new(),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: PackToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test_tool");
        assert_eq!(back.timeout, 45_000);
        assert_eq!(back.groups, vec!["read"]);
        assert_eq!(back.tags, vec!["recon"]);
        assert_eq!(back.reversibility.as_deref(), Some("fully_reversible"));
        assert_eq!(
            back.input_schema.unwrap().properties["query"].property_type,
            "string"
        );
    }

    #[test]
    fn rejects_empty_pack_name() {
        let dir = setup_pack(&[("pack.toml", "name = \"\"\nversion = \"1.0\"\n")]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
        assert!(err.to_string().contains("invalid pack name"), "{err}");
    }

    #[test]
    fn rejects_pack_name_with_invalid_chars() {
        let dir = setup_pack(&[("pack.toml", "name = \"my pack!\"\nversion = \"1.0\"\n")]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
    }

    #[test]
    fn rejects_pack_name_too_long() {
        let long_name = "a".repeat(65);
        let toml = format!("name = \"{long_name}\"\nversion = \"1.0\"\n");
        let dir = setup_pack(&[("pack.toml", &toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
    }

    #[test]
    fn rejects_empty_pack_version() {
        let dir = setup_pack(&[("pack.toml", "name = \"my-pack\"\nversion = \"\"\n")]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
        assert!(err.to_string().contains("version"), "{err}");
    }

    #[test]
    fn aggregates_multiple_validation_problems() {
        // WHY(#5209): one load must report every contract violation at once,
        // not force a fix-load-fix loop over independent problems.
        let toml = r#"
name = "bad pack!"
version = ""

[[tools]]
name = "ok_tool"
description = "Fine"
command = "tools/ok.sh"
timeout = 0

[[tools]]
name = "bad name"
description = "Bad name"
command = "/etc/passwd"

[overlays.analyst]
agency = "godmode"
model = "  "
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        let msg = err.to_string();
        for expected in [
            "invalid pack name",
            "version is an empty string",
            "invalid timeout",
            "invalid tool name",
            "relative path inside the pack root",
            "unknown agency 'godmode'",
            "model override is blank",
        ] {
            assert!(msg.contains(expected), "missing '{expected}' in: {msg}");
        }
    }

    #[test]
    fn rejects_unknown_overlay_agency() {
        let toml = "name = \"agency-pack\"\nversion = \"1.0\"\n\n[overlays.analyst]\nagency = \"superuser\"\n";
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(
            matches!(err, error::Error::InvalidManifest { .. }),
            "expected InvalidManifest, got: {err}"
        );
        assert!(err.to_string().contains("unknown agency"), "{err}");
    }

    #[test]
    fn accepts_known_overlay_agency_levels() {
        for agency in ["unrestricted", "standard", "restricted"] {
            let toml = format!(
                "name = \"agency-pack\"\nversion = \"1.0\"\n\n[overlays.analyst]\nagency = \"{agency}\"\n"
            );
            let dir = setup_pack(&[("pack.toml", &toml)]);
            load_manifest(dir.path())
                .unwrap_or_else(|e| panic!("agency '{agency}' must validate: {e}"));
        }
    }

    #[test]
    fn rejects_blank_prompt_addition() {
        let toml = "name = \"prompt-pack\"\nversion = \"1.0\"\n\n[overlays.analyst]\nsystem_prompt_additions = [\"  \"]\n";
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(err.to_string().contains("blank"), "{err}");
    }

    #[test]
    fn rejects_file_interpolation_in_prompt_addition() {
        let toml = "name = \"prompt-pack\"\nversion = \"1.0\"\n\n[overlays.analyst]\nsystem_prompt_additions = [\"{{file:large.txt}}\"]\n";
        let dir = setup_pack(&[
            ("pack.toml", toml),
            ("large.txt", "content outside the declared prompt cap"),
        ]);
        let err = load_manifest(dir.path()).unwrap_err();
        let display = err.to_string();
        assert!(display.contains("must not contain"), "{display}");
        assert!(display.contains("file"), "{display}");
    }

    #[test]
    fn rejects_unbound_tool_authority_and_invalid_narrowing_fields() {
        // SECURITY(#5214): a manifest cannot self-authorize daemon env or writes. Egress and
        // platform remain declarative narrowing/compatibility fields and are validated here.
        let toml = r#"
name = "policy-pack"
version = "1.0"

[[tools]]
name = "bad_env"
description = "Bad env name"
command = "tools/x.sh"
env = ["HAS=VALUE"]

[[tools]]
name = "bad_write"
description = "Escaping write path"
command = "tools/x.sh"
write_paths = ["../elsewhere"]

[[tools]]
name = "bad_egress"
description = "Unknown egress"
command = "tools/x.sh"
egress = "everything"

[[tools]]
name = "bad_platform"
description = "Unknown platform"
command = "tools/x.sh"
platforms = ["plan9"]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).unwrap_err();
        let msg = err.to_string();
        for expected in [
            "env authority",
            "write_paths authority",
            "egress 'everything' is unknown",
            "platform 'plan9' is unknown",
        ] {
            assert!(msg.contains(expected), "missing '{expected}' in: {msg}");
        }
    }

    #[test]
    fn rejects_even_well_formed_env_and_write_declarations_without_operator_policy() {
        let toml = r#"
name = "policy-pack"
version = "1.0"

[[tools]]
name = "query"
description = "Query with declared policy"
command = "tools/query.sh"
env = ["DATABASE_URL"]
write_paths = ["data", "data/scratch"]
egress = "none"
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).expect_err("pack must not self-grant authority");
        let message = err.to_string();
        assert!(message.contains("env authority"), "{message}");
        assert!(message.contains("write_paths authority"), "{message}");
    }

    #[test]
    fn rejects_windows_platform_until_native_execution_is_supported() {
        let toml = r#"
name = "platform-pack"
version = "1.0"

[[tools]]
name = "native_tool"
description = "Not yet portable"
command = "tools/native.exe"
platforms = ["windows"]
"#;
        let dir = setup_pack(&[("pack.toml", toml)]);
        let err = load_manifest(dir.path()).expect_err("Windows is not a supported pack platform");
        assert!(err.to_string().contains("platform 'windows' is unknown"));
    }

    #[test]
    fn accepts_valid_pack_name_with_hyphens() {
        let dir = setup_pack(&[("pack.toml", "name = \"my-pack-123\"\nversion = \"1.0\"\n")]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name, "my-pack-123");
    }

    #[test]
    fn accepts_pack_name_at_max_length() {
        let name = "a".repeat(64);
        let toml = format!("name = \"{name}\"\nversion = \"1.0\"\n");
        let dir = setup_pack(&[("pack.toml", &toml)]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name.len(), 64);
    }
}
