//! On-disk workspace persistence for projects.

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error::{self, Result};
use crate::plan::Blocker;
use crate::project::Project;

/// Manages the on-disk workspace for a project.
pub struct ProjectWorkspace {
    root: PathBuf,
}

/// Standard directories in a project workspace.
pub struct WorkspaceLayout {
    /// Root directory of the project workspace.
    pub root: PathBuf,
    /// Path to the main `PROJECT.json` file.
    pub project_file: PathBuf,
    /// Directory for phase-specific files.
    pub phases_dir: PathBuf,
    /// Directory for blocker markdown files (under `.dianoia/`).
    pub blockers_dir: PathBuf,
    /// Directory for execution artifacts and outputs.
    pub artifacts_dir: PathBuf,
}

impl ProjectWorkspace {
    /// Create a new workspace at the given path.
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let layout = Self::build_layout(&root);

        for dir in [
            &layout.phases_dir,
            &layout.blockers_dir,
            &layout.artifacts_dir,
        ] {
            std::fs::create_dir_all(dir).context(error::WorkspaceIoSnafu { path: dir.clone() })?;
        }

        Ok(Self { root })
    }

    /// Open an existing workspace.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.exists() {
            return error::ProjectNotFoundSnafu { path: root }.fail();
        }
        Ok(Self { root })
    }

    /// Save project state to disk.
    pub fn save_project(&self, project: &Project) -> Result<()> {
        let layout = self.layout();
        let json = serde_json::to_string_pretty(project).context(error::WorkspaceSerializeSnafu)?;
        std::fs::write(&layout.project_file, json).context(error::WorkspaceIoSnafu {
            path: &layout.project_file,
        })?;
        Ok(())
    }

    /// Load project state from disk.
    pub fn load_project(&self) -> Result<Project> {
        let layout = self.layout();
        if !layout.project_file.exists() {
            return error::ProjectNotFoundSnafu {
                path: layout.project_file,
            }
            .fail();
        }
        let contents =
            std::fs::read_to_string(&layout.project_file).context(error::WorkspaceIoSnafu {
                path: &layout.project_file,
            })?;
        let project: Project =
            serde_json::from_str(&contents).context(error::WorkspaceDeserializeSnafu)?;
        Ok(project)
    }

    /// Write a blocker file for stuck detection integration.
    pub fn write_blocker(&self, phase_id: &str, blocker: &Blocker) -> Result<()> {
        let layout = self.layout();
        let phase_blockers = layout.blockers_dir.join(phase_id);
        std::fs::create_dir_all(&phase_blockers).context(error::WorkspaceIoSnafu {
            path: &phase_blockers,
        })?;

        let filename = format!("{}.md", blocker.plan_id);
        let path = phase_blockers.join(&filename);
        let content = format!(
            "# Blocker: {}\n\nPlan: {}\nDetected: {}\n\n{}\n",
            blocker.plan_id, blocker.plan_id, blocker.detected_at, blocker.description
        );
        std::fs::write(&path, content).context(error::WorkspaceIoSnafu { path })?;
        Ok(())
    }

    /// Read all blockers for a phase.
    pub fn read_blockers(&self, phase_id: &str) -> Result<Vec<Blocker>> {
        let layout = self.layout();
        let phase_blockers = layout.blockers_dir.join(phase_id);

        if !phase_blockers.exists() {
            return Ok(Vec::new());
        }

        let mut blockers = Vec::new();
        let entries = std::fs::read_dir(&phase_blockers).context(error::WorkspaceIoSnafu {
            path: &phase_blockers,
        })?;

        for entry in entries {
            let entry = entry.context(error::WorkspaceIoSnafu {
                path: &phase_blockers,
            })?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                let content = std::fs::read_to_string(&path)
                    .context(error::WorkspaceIoSnafu { path: &path })?;
                let plan_id_str = path.file_stem().unwrap_or_default().to_string_lossy();

                blockers.push(Blocker {
                    description: content,
                    plan_id: plan_id_str
                        .parse::<ulid::Ulid>()
                        .unwrap_or_else(|_| ulid::Ulid::new()),
                    detected_at: jiff::Timestamp::now(),
                });
            }
        }

        Ok(blockers)
    }

    /// Get the workspace directory layout.
    #[must_use]
    pub fn layout(&self) -> WorkspaceLayout {
        Self::build_layout(&self.root)
    }

    fn build_layout(root: &Path) -> WorkspaceLayout {
        WorkspaceLayout {
            root: root.to_path_buf(),
            project_file: root.join("PROJECT.json"),
            phases_dir: root.join("phases"),
            blockers_dir: root.join(".dianoia").join("blockers"),
            artifacts_dir: root.join("artifacts"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectMode;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ProjectWorkspace::create(dir.path().join("project")).unwrap();

        let project = Project::new(
            "roundtrip-test".into(),
            "testing persistence".into(),
            ProjectMode::Full,
            "syn".into(),
        );
        ws.save_project(&project).unwrap();

        let loaded = ws.load_project().unwrap();
        assert_eq!(loaded.id, project.id);
        assert_eq!(loaded.name, project.name);
        assert_eq!(loaded.description, project.description);
        assert_eq!(loaded.owner, project.owner);
    }

    #[test]
    fn blocker_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ProjectWorkspace::create(dir.path().join("project")).unwrap();

        let plan_id = ulid::Ulid::new();
        let blocker = Blocker {
            description: "blocked on API design".into(),
            plan_id,
            detected_at: jiff::Timestamp::now(),
        };

        ws.write_blocker("phase-1", &blocker).unwrap();
        let blockers = ws.read_blockers("phase-1").unwrap();
        assert_eq!(blockers.len(), 1);
        assert!(blockers[0].description.contains("blocked on API design"));
    }

    #[test]
    fn open_nonexistent_returns_error() {
        let result = ProjectWorkspace::open("/nonexistent/workspace/path");
        assert!(result.is_err());
    }
}
