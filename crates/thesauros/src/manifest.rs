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
pub enum Priority {
    Required,
    Important,
    Flexible,
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
/// Reads `{pack_root}/pack.yaml`, validates structure, and returns the parsed manifest.
///
/// # Errors
///
/// - [`error::Error::PackNotFound`] if `pack_root` does not exist
/// - [`error::Error::ManifestNotFound`] if `pack.yaml` is missing
/// - [`error::Error::ReadFile`] if the file cannot be read
/// - [`error::Error::ParseManifest`] if YAML parsing fails
pub fn load_manifest(pack_root: &Path) -> Result<PackManifest> {
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
            location: snafu::Location::new(file!(), line!(), column!()),
        })?;

    Ok(manifest)
}

/// Resolve a context entry path relative to the pack root.
///
/// Returns the canonical absolute path, or an error if the file does not
/// exist or if the resolved path escapes the pack root directory.
pub fn resolve_context_path(pack_root: &Path, entry: &ContextEntry) -> Result<PathBuf> {
    let resolved = pack_root.join(&entry.path);
    ensure!(
        resolved.is_file(),
        error::ContextFileNotFoundSnafu { path: &resolved }
    );

    // WHY: `pack_root.join(path)` does not prevent `../` sequences from
    // escaping the pack directory. Canonicalize resolves all symlinks and
    // parent-dir components, then verify the result is still under the root.
    let canonical = resolved
        .canonicalize()
        .context(error::ReadFileSnafu { path: resolved })?;
    let canonical_root = pack_root.canonicalize().context(error::ReadFileSnafu {
        path: pack_root.to_path_buf(),
    })?;
    ensure!(
        canonical.starts_with(&canonical_root),
        error::ContextFileEscapeSnafu { path: &canonical }
    );

    Ok(canonical)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_pack(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
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
agents = ["chiron"]
truncatable = false

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[[context]]
path = "context/SQL_PATTERNS.md"
priority = "important"

[overlays.chiron]
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
        assert_eq!(manifest.context[0].agents, vec!["chiron"]);
        assert!(!manifest.context[0].truncatable);
        assert_eq!(manifest.context[1].priority, Priority::Flexible);
        assert!(manifest.context[1].truncatable);
        assert!(manifest.context[2].agents.is_empty());

        let chiron = manifest.overlays.get("chiron").unwrap();
        assert_eq!(chiron.domains, vec!["healthcare", "analytics", "sql"]);
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
        // Create the "outer" file that the traversal path would target.
        let outer = TempDir::new().unwrap();
        fs::write(outer.path().join("secret.md"), "secret content").unwrap();

        // Create the pack directory; it has no legitimate files.
        let pack = TempDir::new().unwrap();

        // Build a relative path that tries to escape via `../`.
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
                agents: vec!["chiron".to_owned()],
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
        assert!(manifest.tools[0].input_schema.is_some());
        let schema = manifest.tools[0].input_schema.as_ref().unwrap();
        assert_eq!(schema.required, vec!["sql"]);
        assert_eq!(schema.properties["sql"].property_type, "string");

        // Second tool uses default timeout
        assert_eq!(manifest.tools[1].timeout, 30_000);
    }

    #[test]
    fn manifest_without_tools_backward_compat() {
        let dir = setup_pack(&[("pack.toml", minimal_manifest())]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert!(manifest.tools.is_empty());
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
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: PackToolDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test_tool");
        assert_eq!(back.timeout, 45_000);
        assert_eq!(
            back.input_schema.unwrap().properties["query"].property_type,
            "string"
        );
    }
}
