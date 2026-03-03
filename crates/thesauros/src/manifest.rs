//! Pack manifest parsing and validation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, ensure};

use crate::error::{self, Result};

/// Manifest filename expected in every pack root.
const MANIFEST_FILENAME: &str = "pack.yaml";

/// A parsed and validated domain pack manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    /// Pack name (e.g. "summus-analytics").
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Context files to inject into bootstrap.
    #[serde(default)]
    pub context: Vec<ContextEntry>,
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

    let contents =
        std::fs::read_to_string(&manifest_path).context(error::ReadFileSnafu {
            path: manifest_path.clone(),
        })?;

    let manifest: PackManifest =
        serde_yml::from_str(&contents).map_err(|e| error::Error::ParseManifest {
            path: manifest_path,
            reason: e.to_string(),
            location: snafu::Location::new(file!(), line!(), column!()),
        })?;

    Ok(manifest)
}

/// Resolve a context entry path relative to the pack root.
///
/// Returns the absolute path, or an error if the file does not exist.
pub fn resolve_context_path(pack_root: &Path, entry: &ContextEntry) -> Result<PathBuf> {
    let resolved = pack_root.join(&entry.path);
    ensure!(
        resolved.is_file(),
        error::ContextFileNotFoundSnafu { path: &resolved }
    );
    Ok(resolved)
}

#[cfg(test)]
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
        "name: test-pack\nversion: \"1.0\"\n"
    }

    #[test]
    fn load_minimal_manifest() {
        let dir = setup_pack(&[("pack.yaml", minimal_manifest())]);
        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name, "test-pack");
        assert_eq!(manifest.version, "1.0");
        assert!(manifest.context.is_empty());
        assert!(manifest.overlays.is_empty());
    }

    #[test]
    fn load_full_manifest() {
        let yaml = r#"
name: summus-analytics
version: "1.0"
description: Summus Healthcare analytics domain pack

context:
  - path: context/BUSINESS_LOGIC.md
    priority: important
    agents: [chiron]
    truncatable: false
  - path: context/GLOSSARY.md
    priority: flexible
    truncatable: true
  - path: context/SQL_PATTERNS.md
    priority: important

overlays:
  chiron:
    domains: [healthcare, analytics, sql]
"#;
        let dir = setup_pack(&[
            ("pack.yaml", yaml),
            ("context/BUSINESS_LOGIC.md", "business logic"),
            ("context/GLOSSARY.md", "glossary"),
            ("context/SQL_PATTERNS.md", "patterns"),
        ]);

        let manifest = load_manifest(dir.path()).unwrap();
        assert_eq!(manifest.name, "summus-analytics");
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
    fn load_invalid_yaml() {
        let dir = setup_pack(&[("pack.yaml", "{{{{invalid yaml")]);
        let err = load_manifest(dir.path()).unwrap_err();
        assert!(matches!(err, error::Error::ParseManifest { .. }));
    }

    #[test]
    fn resolve_context_path_found() {
        let dir = setup_pack(&[
            ("pack.yaml", minimal_manifest()),
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
        let dir = setup_pack(&[("pack.yaml", minimal_manifest())]);
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
    fn priority_default_is_important() {
        let yaml = "name: test\nversion: \"1.0\"\ncontext:\n  - path: file.md\n";
        let dir = setup_pack(&[("pack.yaml", yaml), ("file.md", "content")]);
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
            overlays: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: PackManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.context[0].priority, Priority::Flexible);
    }
}
