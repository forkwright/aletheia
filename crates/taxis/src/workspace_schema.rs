//! Config-time validation of agent workspace directory structure.
//!
//! [`WorkspaceSchema`] describes the files and directories that must exist
//! inside an agent's workspace root.  Validate a single workspace with
//! [`WorkspaceSchema::validate`], or check all agents in a loaded config with
//! [`validate_agent_workspaces`].
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use aletheia_taxis::workspace_schema::{WorkspaceSchema, WorkspaceRequirement, RequirementKind};
//!
//! let schema = WorkspaceSchema::standard();
//! schema.validate(Path::new("/srv/aletheia/instance/nous/main")).unwrap();
//! ```

use std::path::{Path, PathBuf};

use snafu::Snafu;

use crate::config::AletheiaConfig;
use crate::oikos::Oikos;

/// The kind of filesystem object a [`WorkspaceRequirement`] must find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequirementKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
}

impl RequirementKind {
    fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }

    fn is_satisfied(self, path: &Path) -> bool {
        match self {
            Self::File => path.is_file(),
            Self::Directory => path.is_dir(),
        }
    }
}

/// A single required entry within an agent workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceRequirement {
    /// Path relative to the workspace root.
    pub path: &'static str,
    /// Whether the path must be a file or a directory.
    pub kind: RequirementKind,
}

/// Validation error returned when workspace schema checks find missing entries.
///
/// Contains all failures so that the operator can fix them in one pass.
#[derive(Debug, Snafu)]
#[snafu(display(
    "workspace schema validation failed for {}:\n  - {}",
    workspace.display(),
    failures.join("\n  - ")
))]
pub struct WorkspaceSchemaError {
    /// Path to the workspace root that failed validation.
    pub workspace: PathBuf,
    /// Human-readable description of each missing entry.
    pub failures: Vec<String>,
    #[snafu(implicit)]
    /// Source location captured by snafu.
    pub location: snafu::Location,
}

/// A typed description of the required structure inside an agent workspace.
///
/// Build one via [`WorkspaceSchema::standard`] or assemble custom requirements
/// with [`WorkspaceSchema::new`] + [`WorkspaceSchema::require`].
#[derive(Debug, Default)]
pub struct WorkspaceSchema {
    requirements: Vec<WorkspaceRequirement>,
}

impl WorkspaceSchema {
    /// Create an empty schema with no requirements.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The standard Aletheia agent workspace schema.
    ///
    /// Requires a `SOUL.md` file containing the agent's identity and system
    /// prompt.  This is the minimum that [`crate::oikos::Oikos`] expects to
    /// find when loading an agent.
    #[must_use]
    pub fn standard() -> Self {
        Self::new().require(WorkspaceRequirement {
            path: "SOUL.md",
            kind: RequirementKind::File,
        })
    }

    /// Add a requirement to the schema (builder-style).
    #[must_use]
    pub fn require(mut self, req: WorkspaceRequirement) -> Self {
        self.requirements.push(req);
        self
    }

    /// Validate `workspace` against all requirements in this schema.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceSchemaError`] listing every missing file or directory
    /// when one or more requirements are not satisfied.
    pub fn validate(&self, workspace: &Path) -> Result<(), WorkspaceSchemaError> {
        let mut failures: Vec<String> = Vec::new();

        for req in &self.requirements {
            let target = workspace.join(req.path);
            if !req.kind.is_satisfied(&target) {
                failures.push(format!(
                    "missing required {} '{}' (expected at {})",
                    req.kind.label(),
                    req.path,
                    target.display()
                ));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            WorkspaceSchemaSnafu {
                workspace: workspace.to_path_buf(),
                failures,
            }
            .fail()
        }
    }
}

/// Validate all agent workspaces declared in `config` against the standard schema.
///
/// For each [`crate::config::NousDefinition`] in `config.agents.list`:
/// - Resolves the workspace path (relative paths are anchored to the instance
///   root; absolute paths are used as-is).
/// - Checks that the workspace directory exists.
/// - Validates the directory against [`WorkspaceSchema::standard`].
///
/// All failures across all agents are collected and returned together.
///
/// # Errors
///
/// Returns [`WorkspaceSchemaError`] when any agent workspace fails validation.
/// The `workspace` field in the error is set to the instance root; individual
/// agent failures appear in `failures`.
pub fn validate_agent_workspaces(
    config: &AletheiaConfig,
    oikos: &Oikos,
) -> Result<(), WorkspaceSchemaError> {
    let schema = WorkspaceSchema::standard();
    let mut all_failures: Vec<String> = Vec::new();

    for agent in &config.agents.list {
        let workspace_path = resolve_workspace_path(&agent.workspace, oikos.root());

        if !workspace_path.is_dir() {
            all_failures.push(format!(
                "agent '{}': workspace directory does not exist: {}\n      \
                 help: create the directory or update workspace in aletheia.toml",
                agent.id,
                workspace_path.display()
            ));
            continue;
        }

        if let Err(e) = schema.validate(&workspace_path) {
            for f in e.failures {
                all_failures.push(format!("agent '{}': {f}", agent.id));
            }
        }
    }

    if all_failures.is_empty() {
        Ok(())
    } else {
        WorkspaceSchemaSnafu {
            workspace: oikos.root().to_path_buf(),
            failures: all_failures,
        }
        .fail()
    }
}

/// Resolve a workspace path from config to an absolute path.
///
/// Relative paths are resolved against `instance_root`.
/// Absolute paths are returned unchanged.
fn resolve_workspace_path(workspace: &str, instance_root: &Path) -> PathBuf {
    let p = Path::new(workspace);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        instance_root.join(workspace)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_workspace(files: &[&str], dirs: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for f in files {
            std::fs::write(tmp.path().join(f), b"").unwrap();
        }
        for d in dirs {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        tmp
    }

    // ── WorkspaceSchema::validate ─────────────────────────────────────────

    #[test]
    fn standard_schema_passes_with_soul_md() {
        let ws = make_workspace(&["SOUL.md"], &[]);
        let schema = WorkspaceSchema::standard();
        assert!(
            schema.validate(ws.path()).is_ok(),
            "standard schema should pass when SOUL.md is present"
        );
    }

    #[test]
    fn standard_schema_fails_when_soul_md_missing() {
        let ws = make_workspace(&[], &[]);
        let schema = WorkspaceSchema::standard();
        let err = schema.validate(ws.path()).unwrap_err();
        assert!(
            err.failures.iter().any(|f| f.contains("SOUL.md")),
            "error should mention SOUL.md: {err:?}"
        );
    }

    #[test]
    fn custom_schema_checks_required_dir() {
        let ws = make_workspace(&[], &["bootstrap"]);
        let schema = WorkspaceSchema::new().require(WorkspaceRequirement {
            path: "bootstrap",
            kind: RequirementKind::Directory,
        });
        assert!(
            schema.validate(ws.path()).is_ok(),
            "schema should pass when bootstrap/ exists"
        );
    }

    #[test]
    fn custom_schema_fails_when_required_dir_missing() {
        let ws = make_workspace(&[], &[]);
        let schema = WorkspaceSchema::new().require(WorkspaceRequirement {
            path: "bootstrap",
            kind: RequirementKind::Directory,
        });
        let err = schema.validate(ws.path()).unwrap_err();
        assert!(
            err.failures.iter().any(|f| f.contains("bootstrap")),
            "error should mention bootstrap: {err:?}"
        );
        assert!(
            err.failures.iter().any(|f| f.contains("directory")),
            "error should say directory: {err:?}"
        );
    }

    #[test]
    fn schema_collects_all_failures() {
        let ws = make_workspace(&[], &[]);
        let schema = WorkspaceSchema::new()
            .require(WorkspaceRequirement {
                path: "SOUL.md",
                kind: RequirementKind::File,
            })
            .require(WorkspaceRequirement {
                path: "bootstrap",
                kind: RequirementKind::Directory,
            });
        let err = schema.validate(ws.path()).unwrap_err();
        assert_eq!(
            err.failures.len(),
            2,
            "both failures should be collected: {err:?}"
        );
    }

    #[test]
    fn empty_schema_always_passes() {
        let ws = make_workspace(&[], &[]);
        assert!(WorkspaceSchema::new().validate(ws.path()).is_ok());
    }

    // ── validate_agent_workspaces ─────────────────────────────────────────

    #[test]
    fn validate_agent_workspaces_passes_when_no_agents_configured() {
        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        let config = AletheiaConfig::default();
        assert!(
            validate_agent_workspaces(&config, &oikos).is_ok(),
            "no agents means no workspace failures"
        );
    }

    #[test]
    fn validate_agent_workspaces_fails_when_workspace_missing() {
        use crate::config::NousDefinition;

        let dir = tempfile::tempdir().unwrap();
        let oikos = Oikos::from_root(dir.path());
        let mut config = AletheiaConfig::default();
        config.agents.list.push(NousDefinition {
            id: "alice".to_owned(),
            name: None,
            model: None,
            workspace: "nous/alice".to_owned(),
            thinking_enabled: None,
            agency: None,
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
            recall: None,
        });

        let err = validate_agent_workspaces(&config, &oikos).unwrap_err();
        assert!(
            err.failures.iter().any(|f| f.contains("alice")),
            "failure should mention agent id: {err:?}"
        );
        assert!(
            err.failures.iter().any(|f| f.contains("does not exist")),
            "failure should say workspace does not exist: {err:?}"
        );
    }

    #[test]
    fn validate_agent_workspaces_fails_when_soul_md_missing() {
        use crate::config::NousDefinition;

        let dir = tempfile::tempdir().unwrap();
        let workspace_dir = dir.path().join("nous").join("bob");
        std::fs::create_dir_all(&workspace_dir).unwrap();
        // NOTE: no SOUL.md in the workspace

        let oikos = Oikos::from_root(dir.path());
        let mut config = AletheiaConfig::default();
        config.agents.list.push(NousDefinition {
            id: "bob".to_owned(),
            name: None,
            model: None,
            workspace: "nous/bob".to_owned(),
            thinking_enabled: None,
            agency: None,
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
            recall: None,
        });

        let err = validate_agent_workspaces(&config, &oikos).unwrap_err();
        assert!(
            err.failures.iter().any(|f| f.contains("bob")),
            "failure should mention agent id: {err:?}"
        );
        assert!(
            err.failures.iter().any(|f| f.contains("SOUL.md")),
            "failure should mention SOUL.md: {err:?}"
        );
    }

    #[test]
    fn validate_agent_workspaces_passes_when_workspace_valid() {
        use crate::config::NousDefinition;

        let dir = tempfile::tempdir().unwrap();
        let workspace_dir = dir.path().join("nous").join("carol");
        std::fs::create_dir_all(&workspace_dir).unwrap();
        std::fs::write(workspace_dir.join("SOUL.md"), b"# Carol\n").unwrap();

        let oikos = Oikos::from_root(dir.path());
        let mut config = AletheiaConfig::default();
        config.agents.list.push(NousDefinition {
            id: "carol".to_owned(),
            name: None,
            model: None,
            workspace: "nous/carol".to_owned(),
            thinking_enabled: None,
            agency: None,
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
            recall: None,
        });

        assert!(
            validate_agent_workspaces(&config, &oikos).is_ok(),
            "valid workspace should pass"
        );
    }
}
