//! Context handoff protocol for continuity across context breaks.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{self, Result};

const HANDOFF_JSON_FILENAME: &str = ".continue-here.json";
const HANDOFF_MD_FILENAME: &str = ".continue-here.md";

/// Why the handoff is being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HandoffReason {
    /// Context distillation is about to compress the conversation.
    Distillation,
    /// The process is shutting down in a controlled manner.
    ControlledShutdown,
    /// The context window is approaching its limit.
    ContextLimitApproaching,
}

/// Full context preserved across a context break.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffContext {
    /// Description of the current task being worked on.
    pub task: String,
    /// Progress made so far (milestones, completed steps).
    pub progress: Vec<String>,
    /// What needs to happen next to continue the work.
    pub next_steps: Vec<String>,
    /// File paths relevant to the current work.
    pub relevant_paths: Vec<PathBuf>,
    /// Partial results or intermediate outputs worth preserving.
    pub partial_results: Vec<String>,
    /// Project identifier, if operating within a project context.
    pub project_id: Option<String>,
    /// Session identifier for the originating session.
    pub session_id: Option<String>,
    /// Why the handoff was triggered.
    pub reason: HandoffReason,
    /// When the handoff was created.
    pub created_at: jiff::Timestamp,
}

impl HandoffContext {
    /// Render the handoff context as a human-readable markdown string.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut md = String::from("# Continue Here\n\n");

        md.push_str("## Task\n\n");
        md.push_str(&self.task);
        md.push_str("\n\n");

        if !self.progress.is_empty() {
            md.push_str("## Progress\n\n");
            for item in &self.progress {
                md.push_str("- ");
                md.push_str(item);
                md.push('\n');
            }
            md.push('\n');
        }

        if !self.next_steps.is_empty() {
            md.push_str("## Next steps\n\n");
            for step in &self.next_steps {
                md.push_str("- ");
                md.push_str(step);
                md.push('\n');
            }
            md.push('\n');
        }

        if !self.relevant_paths.is_empty() {
            md.push_str("## Relevant files\n\n");
            for path in &self.relevant_paths {
                md.push_str("- `");
                md.push_str(&path.display().to_string());
                md.push_str("`\n");
            }
            md.push('\n');
        }

        if !self.partial_results.is_empty() {
            md.push_str("## Partial results\n\n");
            for result in &self.partial_results {
                md.push_str("- ");
                md.push_str(result);
                md.push('\n');
            }
            md.push('\n');
        }

        md.push_str("## Metadata\n\n");
        if let Some(project_id) = &self.project_id {
            md.push_str("- Project: ");
            md.push_str(project_id);
            md.push('\n');
        }
        if let Some(session_id) = &self.session_id {
            md.push_str("- Session: ");
            md.push_str(session_id);
            md.push('\n');
        }
        md.push_str("- Reason: ");
        md.push_str(match self.reason {
            HandoffReason::Distillation => "distillation",
            HandoffReason::ControlledShutdown => "controlled shutdown",
            HandoffReason::ContextLimitApproaching => "context limit approaching",
        });
        md.push('\n');
        md.push_str("- Created: ");
        md.push_str(&self.created_at.to_string());
        md.push('\n');

        md
    }
}

/// Manages handoff file I/O for context continuity across breaks.
///
/// Writes both a machine-readable JSON file (`.continue-here.json`) and a
/// human-readable markdown file (`.continue-here.md`) at the given directory.
pub struct HandoffFile {
    dir: PathBuf,
}

impl HandoffFile {
    /// Create a handoff file manager for the given directory.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Write a handoff file before distillation or controlled shutdown.
    ///
    /// Creates both `.continue-here.json` (machine-readable) and
    /// `.continue-here.md` (human-readable) in the configured directory.
    pub fn write(&self, context: &HandoffContext) -> Result<()> {
        let json_path = self.json_path();
        let md_path = self.md_path();

        let json = serde_json::to_string_pretty(context).context(error::HandoffSerializeSnafu)?;
        let markdown = context.to_markdown();

        #[expect(
            clippy::disallowed_methods,
            reason = "handoff writes are pre-shutdown blocking operations; async context unavailable"
        )]
        {
            std::fs::create_dir_all(&self.dir).context(error::HandoffIoSnafu {
                path: self.dir.clone(),
            })?;
            std::fs::write(&json_path, json).context(error::HandoffIoSnafu { path: &json_path })?;
            std::fs::write(&md_path, markdown).context(error::HandoffIoSnafu { path: &md_path })?;
        }
        // WHY: restrict handoff files to owner-only (0600) — contain session context
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&json_path, std::fs::Permissions::from_mode(0o600))
                .context(error::HandoffIoSnafu { path: &json_path })?;
            std::fs::set_permissions(&md_path, std::fs::Permissions::from_mode(0o600))
                .context(error::HandoffIoSnafu { path: &md_path })?;
        }

        Ok(())
    }

    /// Read a handoff file on resume, returning the preserved context.
    ///
    /// Returns `Ok(None)` if no handoff file exists.
    pub fn read(&self) -> Result<Option<HandoffContext>> {
        let json_path = self.json_path();
        if !json_path.exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&json_path)
            .context(error::HandoffIoSnafu { path: &json_path })?;
        let context: HandoffContext =
            serde_json::from_str(&contents).context(error::HandoffDeserializeSnafu)?;

        Ok(Some(context))
    }

    /// Check whether an orphaned handoff file exists from a previous session.
    ///
    /// An existing handoff file at startup indicates either a planned handoff
    /// or a crash recovery scenario. The caller decides based on session context.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.json_path().exists()
    }

    /// Remove handoff files after a successful resume.
    pub fn clear(&self) -> Result<()> {
        let json_path = self.json_path();
        let md_path = self.md_path();

        if json_path.exists() {
            std::fs::remove_file(&json_path).context(error::HandoffIoSnafu { path: &json_path })?;
        }
        if md_path.exists() {
            std::fs::remove_file(&md_path).context(error::HandoffIoSnafu { path: &md_path })?;
        }

        Ok(())
    }

    /// Path to the JSON handoff file.
    #[must_use]
    pub fn json_path(&self) -> PathBuf {
        self.dir.join(HANDOFF_JSON_FILENAME)
    }

    /// Path to the markdown handoff file.
    #[must_use]
    pub fn md_path(&self) -> PathBuf {
        self.dir.join(HANDOFF_MD_FILENAME)
    }
}

impl std::fmt::Debug for HandoffFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandoffFile")
            .field("dir", &self.dir)
            .finish()
    }
}

/// Detect an orphaned handoff file at a given path.
///
/// Returns `Ok(Some(context))` if a handoff file exists (indicating a crash or
/// incomplete resume from a prior session). Returns `Ok(None)` if no handoff exists.
pub fn detect_orphaned(dir: &Path) -> Result<Option<HandoffContext>> {
    HandoffFile::new(dir).read()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn sample_context() -> HandoffContext {
        HandoffContext {
            task: "Implement stuck detection for dianoia".into(),
            progress: vec![
                "Created StuckDetector struct".into(),
                "Implemented repeated error detection".into(),
            ],
            next_steps: vec![
                "Add alternating failure detection".into(),
                "Write unit tests".into(),
            ],
            relevant_paths: vec![
                PathBuf::from("crates/dianoia/src/stuck.rs"),
                PathBuf::from("crates/dianoia/src/lib.rs"),
            ],
            partial_results: vec!["StuckDetector compiles and passes 3 tests".into()],
            project_id: Some("01JTEST00000000000000000".into()),
            session_id: Some("01JSESS00000000000000000".into()),
            reason: HandoffReason::Distillation,
            created_at: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());

        let context = sample_context();
        handoff.write(&context).unwrap();

        assert!(handoff.exists());

        let loaded = handoff.read().unwrap().unwrap();
        assert_eq!(loaded.task, context.task);
        assert_eq!(loaded.progress, context.progress);
        assert_eq!(loaded.next_steps, context.next_steps);
        assert_eq!(loaded.relevant_paths, context.relevant_paths);
        assert_eq!(loaded.partial_results, context.partial_results);
        assert_eq!(loaded.project_id, context.project_id);
        assert_eq!(loaded.session_id, context.session_id);
        assert_eq!(loaded.reason, context.reason);
        assert_eq!(loaded.created_at, context.created_at);
    }

    #[test]
    fn read_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());
        let result = handoff.read().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn exists_before_and_after_write() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());

        assert!(!handoff.exists());
        handoff.write(&sample_context()).unwrap();
        assert!(handoff.exists());
    }

    #[test]
    fn clear_removes_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());

        handoff.write(&sample_context()).unwrap();
        assert!(handoff.json_path().exists());
        assert!(handoff.md_path().exists());

        handoff.clear().unwrap();
        assert!(!handoff.json_path().exists());
        assert!(!handoff.md_path().exists());
    }

    #[test]
    fn clear_nonexistent_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());
        handoff.clear().unwrap();
    }

    #[test]
    fn detect_orphaned_finds_existing() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());
        handoff.write(&sample_context()).unwrap();

        let orphaned = detect_orphaned(dir.path()).unwrap();
        assert!(orphaned.is_some());
        assert_eq!(
            orphaned.unwrap().task,
            "Implement stuck detection for dianoia"
        );
    }

    #[test]
    fn detect_orphaned_returns_none_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = detect_orphaned(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn markdown_contains_task() {
        let context = sample_context();
        let md = context.to_markdown();
        assert!(md.contains("# Continue Here"));
        assert!(md.contains("## Task"));
        assert!(md.contains("Implement stuck detection for dianoia"));
    }

    #[test]
    fn markdown_contains_progress() {
        let context = sample_context();
        let md = context.to_markdown();
        assert!(md.contains("## Progress"));
        assert!(md.contains("- Created StuckDetector struct"));
        assert!(md.contains("- Implemented repeated error detection"));
    }

    #[test]
    fn markdown_contains_next_steps() {
        let context = sample_context();
        let md = context.to_markdown();
        assert!(md.contains("## Next steps"));
        assert!(md.contains("- Add alternating failure detection"));
    }

    #[test]
    fn markdown_contains_relevant_files() {
        let context = sample_context();
        let md = context.to_markdown();
        assert!(md.contains("## Relevant files"));
        assert!(md.contains("`crates/dianoia/src/stuck.rs`"));
    }

    #[test]
    fn markdown_contains_metadata() {
        let context = sample_context();
        let md = context.to_markdown();
        assert!(md.contains("## Metadata"));
        assert!(md.contains("- Project: 01JTEST00000000000000000"));
        assert!(md.contains("- Session: 01JSESS00000000000000000"));
        assert!(md.contains("- Reason: distillation"));
    }

    #[test]
    fn markdown_omits_empty_sections() {
        let context = HandoffContext {
            task: "minimal task".into(),
            progress: Vec::new(),
            next_steps: Vec::new(),
            relevant_paths: Vec::new(),
            partial_results: Vec::new(),
            project_id: None,
            session_id: None,
            reason: HandoffReason::ControlledShutdown,
            created_at: jiff::Timestamp::now(),
        };
        let md = context.to_markdown();
        assert!(md.contains("## Task"));
        assert!(!md.contains("## Progress"));
        assert!(!md.contains("## Next steps"));
        assert!(!md.contains("## Relevant files"));
        assert!(!md.contains("## Partial results"));
        assert!(md.contains("- Reason: controlled shutdown"));
    }

    #[test]
    fn markdown_file_written_alongside_json() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());
        handoff.write(&sample_context()).unwrap();

        assert!(handoff.md_path().exists());
        let md_content = std::fs::read_to_string(handoff.md_path()).unwrap();
        assert!(md_content.contains("# Continue Here"));
    }

    #[test]
    fn write_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("nested").join("dir");
        let handoff = HandoffFile::new(&nested);

        handoff.write(&sample_context()).unwrap();
        assert!(handoff.exists());
    }

    #[test]
    fn overwrite_replaces_previous_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let handoff = HandoffFile::new(dir.path());

        let first = sample_context();
        handoff.write(&first).unwrap();

        let mut second = sample_context();
        second.task = "Updated task description".into();
        handoff.write(&second).unwrap();

        let loaded = handoff.read().unwrap().unwrap();
        assert_eq!(loaded.task, "Updated task description");
    }

    #[test]
    fn context_serde_roundtrip() {
        let context = sample_context();
        let json = serde_json::to_string(&context).unwrap();
        let back: HandoffContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task, context.task);
        assert_eq!(back.reason, context.reason);
    }

    #[test]
    fn handoff_reason_serde_roundtrip() {
        let reasons = [
            HandoffReason::Distillation,
            HandoffReason::ControlledShutdown,
            HandoffReason::ContextLimitApproaching,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).unwrap();
            let back: HandoffReason = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, reason, "roundtrip failed for {reason:?}");
        }
    }
}
