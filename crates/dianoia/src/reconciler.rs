//! State reconciler: keeps planning state consistent between two sources.
//!
//! Both database and filesystem are authoritative. On startup (or periodically),
//! the reconciler compares two project snapshots and determines which direction
//! to sync. Conflicts are logged for manual review.

use serde::{Deserialize, Serialize};

use crate::project::Project;

/// A project snapshot from a single source, tagged with its origin and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    /// The project state from this source.
    pub project: Project,
    /// Which source this snapshot came from.
    pub origin: SnapshotOrigin,
}

/// Where a project snapshot was loaded from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SnapshotOrigin {
    /// Loaded from the database.
    Database,
    /// Loaded from the filesystem (workspace JSON).
    Filesystem,
}

impl std::fmt::Display for SnapshotOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database => f.write_str("database"),
            Self::Filesystem => f.write_str("filesystem"),
        }
    }
}

/// Direction of reconciliation for a single project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReconciliationDirection {
    /// Database is newer; regenerate filesystem from DB.
    DbToFiles,
    /// Filesystem is newer; import into DB.
    FilesToDb,
    /// Both sources are in sync (within tolerance).
    InSync,
    /// Project exists only in the database.
    DbOnly,
    /// Project exists only on the filesystem.
    FilesOnly,
}

impl std::fmt::Display for ReconciliationDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DbToFiles => f.write_str("db-to-files"),
            Self::FilesToDb => f.write_str("files-to-db"),
            Self::InSync => f.write_str("in-sync"),
            Self::DbOnly => f.write_str("db-only"),
            Self::FilesOnly => f.write_str("files-only"),
        }
    }
}

/// A conflict detected during reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    /// Which field diverged between the two sources.
    pub field: String,
    /// Value from the database source.
    pub db_value: String,
    /// Value from the filesystem source.
    pub fs_value: String,
    /// Which source won (newest-wins).
    pub resolution: SnapshotOrigin,
}

/// Result of reconciling a single project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationResult {
    /// The project ID that was reconciled.
    pub project_id: String,
    /// Which direction the sync went.
    pub direction: ReconciliationDirection,
    /// Conflicts detected between the two sources.
    pub conflicts: Vec<ConflictEntry>,
    /// The winning project state after reconciliation.
    pub resolved: Option<Project>,
    /// Errors encountered during reconciliation.
    pub errors: Vec<String>,
}

/// Summary of reconciling all known projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationSummary {
    /// Per-project results.
    pub projects: Vec<ReconciliationResult>,
    /// Total number of errors across all projects.
    pub total_errors: usize,
    /// Total number of conflicts detected.
    pub total_conflicts: usize,
}

/// Tolerance in seconds for timestamp comparison. Differences within this
/// window are treated as "in sync."
const TIMESTAMP_TOLERANCE_SECS: i64 = 5;

/// Reconcile two optional project snapshots into a single result.
///
/// Both database and filesystem snapshots are optional: a project may exist
/// in only one source. When both exist, the newest-wins strategy applies
/// and conflicts are logged.
#[must_use]
pub fn reconcile(
    db_snapshot: Option<&ProjectSnapshot>,
    fs_snapshot: Option<&ProjectSnapshot>,
) -> ReconciliationResult {
    match (db_snapshot, fs_snapshot) {
        (Some(db), None) => ReconciliationResult {
            project_id: db.project.id.to_string(),
            direction: ReconciliationDirection::DbOnly,
            conflicts: Vec::new(),
            resolved: Some(db.project.clone()),
            errors: Vec::new(),
        },

        (None, Some(fs)) => ReconciliationResult {
            project_id: fs.project.id.to_string(),
            direction: ReconciliationDirection::FilesOnly,
            conflicts: Vec::new(),
            resolved: Some(fs.project.clone()),
            errors: Vec::new(),
        },

        (None, None) => ReconciliationResult {
            project_id: String::new(),
            direction: ReconciliationDirection::InSync,
            conflicts: Vec::new(),
            resolved: None,
            errors: vec!["no snapshots provided".to_owned()],
        },

        (Some(db), Some(fs)) => reconcile_both(db, fs),
    }
}

/// Reconcile when both sources have the project.
fn reconcile_both(db: &ProjectSnapshot, fs: &ProjectSnapshot) -> ReconciliationResult {
    let project_id = db.project.id.to_string();
    let mut conflicts = Vec::new();

    detect_conflicts(&db.project, &fs.project, &mut conflicts);

    let diff_secs = db
        .project
        .updated_at
        .as_second()
        .saturating_sub(fs.project.updated_at.as_second());

    let (direction, winner) = if diff_secs > TIMESTAMP_TOLERANCE_SECS {
        (ReconciliationDirection::DbToFiles, SnapshotOrigin::Database)
    } else if diff_secs < -TIMESTAMP_TOLERANCE_SECS {
        (
            ReconciliationDirection::FilesToDb,
            SnapshotOrigin::Filesystem,
        )
    } else {
        // WHY: within tolerance, prefer DB as canonical source
        (ReconciliationDirection::InSync, SnapshotOrigin::Database)
    };

    for conflict in &mut conflicts {
        conflict.resolution = winner;
    }

    let resolved = match winner {
        SnapshotOrigin::Database => db.project.clone(),
        SnapshotOrigin::Filesystem => fs.project.clone(),
    };

    ReconciliationResult {
        project_id,
        direction,
        conflicts,
        resolved: Some(resolved),
        errors: Vec::new(),
    }
}

/// Compare two projects and log field-level conflicts.
fn detect_conflicts(db: &Project, fs: &Project, conflicts: &mut Vec<ConflictEntry>) {
    if db.name != fs.name {
        conflicts.push(ConflictEntry {
            field: "name".to_owned(),
            db_value: db.name.clone(),
            fs_value: fs.name.clone(),
            resolution: SnapshotOrigin::Database,
        });
    }

    if db.description != fs.description {
        conflicts.push(ConflictEntry {
            field: "description".to_owned(),
            db_value: db.description.clone(),
            fs_value: fs.description.clone(),
            resolution: SnapshotOrigin::Database,
        });
    }

    if db.state != fs.state {
        conflicts.push(ConflictEntry {
            field: "state".to_owned(),
            db_value: format!("{:?}", db.state),
            fs_value: format!("{:?}", fs.state),
            resolution: SnapshotOrigin::Database,
        });
    }

    if db.phases.len() != fs.phases.len() {
        conflicts.push(ConflictEntry {
            field: "phases.len".to_owned(),
            db_value: db.phases.len().to_string(),
            fs_value: fs.phases.len().to_string(),
            resolution: SnapshotOrigin::Database,
        });
    }
}

/// Reconcile multiple projects from two source sets.
///
/// Matches projects by ID across both sets and produces a summary.
#[must_use]
pub fn reconcile_all(
    db_snapshots: &[ProjectSnapshot],
    fs_snapshots: &[ProjectSnapshot],
) -> ReconciliationSummary {
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();

    let db_by_id: std::collections::HashMap<_, _> =
        db_snapshots.iter().map(|s| (s.project.id, s)).collect();
    let fs_by_id: std::collections::HashMap<_, _> =
        fs_snapshots.iter().map(|s| (s.project.id, s)).collect();

    for id in db_by_id.keys().chain(fs_by_id.keys()) {
        if !seen.insert(*id) {
            continue;
        }
        let db = db_by_id.get(id).copied();
        let fs = fs_by_id.get(id).copied();
        results.push(reconcile(db, fs));
    }

    let total_errors: usize = results.iter().map(|r| r.errors.len()).sum();
    let total_conflicts: usize = results.iter().map(|r| r.conflicts.len()).sum();

    ReconciliationSummary {
        projects: results,
        total_errors,
        total_conflicts,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::items_after_statements,
    reason = "test-local imports near usage"
)]
mod tests {
    use super::*;
    use crate::project::ProjectMode;
    use crate::state::ProjectState;

    fn make_project(name: &str) -> Project {
        Project::new(
            name.to_owned(),
            format!("{name} description"),
            ProjectMode::Full,
            "alice".to_owned(),
        )
    }

    fn make_snapshot(project: Project, origin: SnapshotOrigin) -> ProjectSnapshot {
        ProjectSnapshot { project, origin }
    }

    #[test]
    fn db_only_project_resolves_to_db() {
        let project = make_project("db-only");
        let snap = make_snapshot(project.clone(), SnapshotOrigin::Database);

        let result = reconcile(Some(&snap), None);

        assert_eq!(result.direction, ReconciliationDirection::DbOnly);
        assert!(result.conflicts.is_empty());
        assert_eq!(result.resolved.as_ref().unwrap().id, project.id);
    }

    #[test]
    fn fs_only_project_resolves_to_fs() {
        let project = make_project("fs-only");
        let snap = make_snapshot(project.clone(), SnapshotOrigin::Filesystem);

        let result = reconcile(None, Some(&snap));

        assert_eq!(result.direction, ReconciliationDirection::FilesOnly);
        assert!(result.conflicts.is_empty());
        assert_eq!(result.resolved.as_ref().unwrap().id, project.id);
    }

    #[test]
    fn no_snapshots_returns_error() {
        let result = reconcile(None, None);

        assert_eq!(result.direction, ReconciliationDirection::InSync);
        assert!(!result.errors.is_empty());
        assert!(result.resolved.is_none());
    }

    #[test]
    fn identical_projects_are_in_sync() {
        let project = make_project("synced");
        let db = make_snapshot(project.clone(), SnapshotOrigin::Database);
        let fs = make_snapshot(project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        assert_eq!(result.direction, ReconciliationDirection::InSync);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn db_newer_wins() {
        let mut db_project = make_project("newer-db");
        // WHY: advance timestamp beyond tolerance to trigger db-to-files
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();
        let fs_project = make_project("newer-db");

        let db = ProjectSnapshot {
            project: db_project.clone(),
            origin: SnapshotOrigin::Database,
        };
        let fs = ProjectSnapshot {
            project: fs_project,
            origin: SnapshotOrigin::Filesystem,
        };

        let result = reconcile(Some(&db), Some(&fs));

        assert_eq!(result.direction, ReconciliationDirection::DbToFiles);
        assert_eq!(
            result.resolved.as_ref().unwrap().updated_at,
            db_project.updated_at
        );
    }

    #[test]
    fn fs_newer_wins() {
        let db_project = make_project("newer-fs");
        let mut fs_project = make_project("newer-fs");
        fs_project.updated_at = fs_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db = ProjectSnapshot {
            project: db_project,
            origin: SnapshotOrigin::Database,
        };
        let fs = ProjectSnapshot {
            project: fs_project.clone(),
            origin: SnapshotOrigin::Filesystem,
        };

        let result = reconcile(Some(&db), Some(&fs));

        assert_eq!(result.direction, ReconciliationDirection::FilesToDb);
        assert_eq!(
            result.resolved.as_ref().unwrap().updated_at,
            fs_project.updated_at
        );
    }

    #[test]
    fn detects_name_conflict() {
        let mut db_project = make_project("db-name");
        let mut fs_project = make_project("fs-name");

        // WHY: set same ID so reconciler treats them as the same project
        fs_project.id = db_project.id;
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db = make_snapshot(db_project, SnapshotOrigin::Database);
        let fs = make_snapshot(fs_project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        assert!(!result.conflicts.is_empty());
        let name_conflict = result.conflicts.iter().find(|c| c.field == "name").unwrap();
        assert_eq!(name_conflict.db_value, "db-name");
        assert_eq!(name_conflict.fs_value, "fs-name");
        assert_eq!(name_conflict.resolution, SnapshotOrigin::Database);
    }

    #[test]
    fn detects_state_conflict() {
        let mut db_project = make_project("state-conflict");
        let mut fs_project = make_project("state-conflict");
        fs_project.id = db_project.id;
        db_project.state = ProjectState::Executing;
        fs_project.state = ProjectState::Researching;
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db = make_snapshot(db_project, SnapshotOrigin::Database);
        let fs = make_snapshot(fs_project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        let state_conflict = result
            .conflicts
            .iter()
            .find(|c| c.field == "state")
            .unwrap();
        assert!(state_conflict.db_value.contains("Executing"));
        assert!(state_conflict.fs_value.contains("Researching"));
    }

    #[test]
    fn detects_phase_count_conflict() {
        let mut db_project = make_project("phase-count");
        let mut fs_project = make_project("phase-count");
        fs_project.id = db_project.id;

        use crate::phase::Phase;
        db_project.add_phase(Phase::new("P1".to_owned(), "g1".to_owned(), 1));
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db = make_snapshot(db_project, SnapshotOrigin::Database);
        let fs = make_snapshot(fs_project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        let phase_conflict = result
            .conflicts
            .iter()
            .find(|c| c.field == "phases.len")
            .unwrap();
        assert_eq!(phase_conflict.db_value, "1");
        assert_eq!(phase_conflict.fs_value, "0");
    }

    #[test]
    fn reconcile_all_matches_by_id() {
        let p1 = make_project("project-1");
        let p2 = make_project("project-2");
        let p3 = make_project("project-3");

        let db_snaps = vec![
            make_snapshot(p1.clone(), SnapshotOrigin::Database),
            make_snapshot(p2.clone(), SnapshotOrigin::Database),
        ];
        let fs_snaps = vec![
            make_snapshot(p2, SnapshotOrigin::Filesystem),
            make_snapshot(p3, SnapshotOrigin::Filesystem),
        ];

        let summary = reconcile_all(&db_snaps, &fs_snaps);

        assert_eq!(summary.projects.len(), 3);

        let p1_result = summary
            .projects
            .iter()
            .find(|r| r.project_id == p1.id.to_string())
            .unwrap();
        assert_eq!(p1_result.direction, ReconciliationDirection::DbOnly);
    }

    #[test]
    fn within_tolerance_treated_as_in_sync() {
        let mut db_project = make_project("tolerance");
        let mut fs_project = make_project("tolerance");
        fs_project.id = db_project.id;

        // WHY: 3 seconds is within the 5-second tolerance window
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(3))
            .unwrap();

        let db = make_snapshot(db_project, SnapshotOrigin::Database);
        let fs = make_snapshot(fs_project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        assert_eq!(result.direction, ReconciliationDirection::InSync);
    }

    #[test]
    fn fs_wins_conflict_resolution_when_newer() {
        let mut db_project = make_project("fs-wins");
        let mut fs_project = make_project("fs-wins-modified");
        fs_project.id = db_project.id;
        db_project.name = "fs-wins".to_owned();
        fs_project.name = "fs-wins-modified".to_owned();

        fs_project.updated_at = fs_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db = make_snapshot(db_project, SnapshotOrigin::Database);
        let fs = make_snapshot(fs_project, SnapshotOrigin::Filesystem);

        let result = reconcile(Some(&db), Some(&fs));

        assert_eq!(result.direction, ReconciliationDirection::FilesToDb);
        let name_conflict = result.conflicts.iter().find(|c| c.field == "name").unwrap();
        assert_eq!(name_conflict.resolution, SnapshotOrigin::Filesystem);
    }

    #[test]
    fn reconcile_all_empty_inputs() {
        let summary = reconcile_all(&[], &[]);

        assert!(summary.projects.is_empty());
        assert_eq!(summary.total_errors, 0);
        assert_eq!(summary.total_conflicts, 0);
    }

    #[test]
    fn reconcile_all_counts_errors_and_conflicts() {
        let mut db_project = make_project("counted");
        let mut fs_project = make_project("counted-different");
        fs_project.id = db_project.id;
        db_project.updated_at = db_project
            .updated_at
            .checked_add(jiff::SignedDuration::from_secs(10))
            .unwrap();

        let db_snaps = vec![make_snapshot(db_project, SnapshotOrigin::Database)];
        let fs_snaps = vec![make_snapshot(fs_project, SnapshotOrigin::Filesystem)];

        let summary = reconcile_all(&db_snaps, &fs_snaps);

        assert_eq!(summary.total_errors, 0);
        assert!(
            summary.total_conflicts > 0,
            "expected conflicts from name/description divergence"
        );
    }

    #[test]
    fn snapshot_origin_display() {
        assert_eq!(SnapshotOrigin::Database.to_string(), "database");
        assert_eq!(SnapshotOrigin::Filesystem.to_string(), "filesystem");
    }

    #[test]
    fn reconciliation_direction_display() {
        assert_eq!(
            ReconciliationDirection::DbToFiles.to_string(),
            "db-to-files"
        );
        assert_eq!(
            ReconciliationDirection::FilesToDb.to_string(),
            "files-to-db"
        );
        assert_eq!(ReconciliationDirection::InSync.to_string(), "in-sync");
        assert_eq!(ReconciliationDirection::DbOnly.to_string(), "db-only");
        assert_eq!(ReconciliationDirection::FilesOnly.to_string(), "files-only");
    }
}
