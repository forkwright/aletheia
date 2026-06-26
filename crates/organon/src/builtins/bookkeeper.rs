//! Bookkeeper tools for prompt archival and worktree cleanup.
//!
//! - `tamias` (ταμίας — steward/treasurer): placeholder for prompt archival
//! - `katharos` (καθαρός — clean): conservative stale worktree cleanup

use std::future::Future;
use std::io::Read as _;
use std::pin::Pin;
use std::process::{Command, Stdio}; // kanon:ignore RUST/no-direct-process-command
use std::time::{Duration, Instant, SystemTime};

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::process_guard::ProcessGuard;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolGroupId, ToolInput, ToolResult, ToolTag,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_GIT_OUTPUT: usize = 256 * 1024;

/// Stub executor for deferred bookkeeper tools.
///
/// WHY: `tamias` still requires integration with the dispatch store. The
/// schema is registered now so deployments can trial the future tool surface.
struct BookkeeperStub {
    tool_name: &'static str,
}

impl ToolExecutor for BookkeeperStub {
    fn execute<'a>(
        &'a self,
        _input: &'a ToolInput,
        _ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        let name = self.tool_name;
        Box::pin(async move {
            tracing::warn!(tool = name, "bookkeeper tool invoked before implementation");
            Ok(ToolResult::error(format!(
                "bookkeeper: {name} is not yet implemented"
            )))
        })
    }
}

struct KatharosExecutor;

impl ToolExecutor for KatharosExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move { execute_katharos(input, ctx) })
    }
}

// -- tamias (ταμίας -- steward/treasurer) ----------------------------------

fn tamias_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("tamias"),
        description: "Placeholder for future prompt archival; currently returns not implemented."
            .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "prompt_number".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Specific prompt number for the future archive operation"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project slug for the future archive operation".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "batch".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Future batch mode flag (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Future dry-run flag (default: false)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::PartiallyReversible,
        auto_activate: false,
        groups: vec![ToolGroupId::Read, ToolGroupId::Verify],
        tags: vec![ToolTag::Edit],
    }
}

// -- katharos (καθαρός -- clean) -------------------------------------------

fn katharos_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("katharos"),
        description: "Clean stale linked git worktrees for the current project.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "project".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Project slug; must match the current repository directory"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "max_age_hours".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Integer,
                        description: "Minimum worktree age in hours before cleanup (default: 48)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(48)),
                        ..Default::default()
                    },
                ),
                (
                    "dry_run".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description:
                            "Report candidate removals without deleting worktrees (default: false)"
                                .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["project".to_owned()],
        },
        category: ToolCategory::System,
        reversibility: Reversibility::Irreversible,
        auto_activate: false,
        groups: vec![
            ToolGroupId::Read,
            ToolGroupId::Edit,
            ToolGroupId::Command,
            ToolGroupId::Verify,
        ],
        tags: vec![ToolTag::Edit, ToolTag::Execute],
    }
}

#[derive(Debug)]
struct WorktreeEntry {
    path: std::path::PathBuf,
    prunable: bool,
}

#[derive(Debug)]
enum CleanupAction {
    Removed(std::path::PathBuf),
    Pruned(std::path::PathBuf),
    WouldRemove(std::path::PathBuf),
    SkippedDirty(std::path::PathBuf),
    SkippedYoung(std::path::PathBuf),
    SkippedOutsideAllowedRoot(std::path::PathBuf),
}

fn execute_katharos(input: &ToolInput, ctx: &ToolContext) -> Result<ToolResult> {
    let project = super::workspace::extract_str(&input.arguments, "project", &input.name)?;
    let max_age_hours =
        super::workspace::extract_opt_u64(&input.arguments, "max_age_hours").unwrap_or(48);
    let dry_run = super::workspace::extract_opt_bool(&input.arguments, "dry_run").unwrap_or(false);

    if !project_matches_workspace(project, ctx) {
        return Ok(ToolResult::error(format!(
            "katharos project mismatch: requested {project:?}, current workspace is {:?}",
            ctx.workspace.file_name().and_then(std::ffi::OsStr::to_str)
        )));
    }

    let worktrees = match list_worktrees(&ctx.workspace) {
        Ok(entries) => entries,
        Err(message) => return Ok(ToolResult::error(message)),
    };

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_hours.saturating_mul(60 * 60)))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let active_workspace = canonicalize_or_clone(&ctx.workspace);
    let mut actions = Vec::new();

    for entry in worktrees {
        let worktree_path = canonicalize_or_clone(&entry.path);
        if worktree_path == active_workspace {
            continue;
        }

        if !is_under_allowed_root(&worktree_path, &ctx.allowed_roots) {
            actions.push(CleanupAction::SkippedOutsideAllowedRoot(entry.path));
            continue;
        }

        if !entry.prunable && !is_old_enough(&worktree_path, cutoff) {
            actions.push(CleanupAction::SkippedYoung(entry.path));
            continue;
        }

        if !entry.prunable && !worktree_is_clean(&worktree_path) {
            actions.push(CleanupAction::SkippedDirty(entry.path));
            continue;
        }

        if dry_run {
            actions.push(CleanupAction::WouldRemove(entry.path));
            continue;
        }

        if entry.prunable {
            actions.push(CleanupAction::Pruned(entry.path));
            continue;
        }

        match remove_worktree(&ctx.workspace, &entry.path) {
            Ok(()) => actions.push(CleanupAction::Removed(entry.path)),
            Err(message) => return Ok(ToolResult::error(message)),
        }
    }

    if !dry_run
        && actions
            .iter()
            .any(|action| matches!(action, CleanupAction::Pruned(_)))
    {
        match run_git(&ctx.workspace, &["worktree", "prune", "--expire", "now"]) {
            Ok(_) => {}
            Err(message) => return Ok(ToolResult::error(message)),
        }
    }

    Ok(ToolResult::text(format_cleanup_actions(&actions, dry_run)))
}

fn project_matches_workspace(project: &str, ctx: &ToolContext) -> bool {
    ctx.workspace
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name == project)
}

fn list_worktrees(repo: &std::path::Path) -> std::result::Result<Vec<WorktreeEntry>, String> {
    let output = run_git(repo, &["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: std::path::PathBuf::from(path),
                prunable: false,
            });
        } else if line.starts_with("prunable ")
            && let Some(entry) = current.as_mut()
        {
            entry.prunable = true;
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    Ok(entries)
}

fn is_old_enough(path: &std::path::Path, cutoff: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified <= cutoff)
}

fn worktree_is_clean(path: &std::path::Path) -> bool {
    run_git(path, &["status", "--porcelain"])
        .map(|output| output.trim().is_empty())
        .unwrap_or(false)
}

fn remove_worktree(
    repo: &std::path::Path,
    path: &std::path::Path,
) -> std::result::Result<(), String> {
    run_git(repo, &["worktree", "remove", path_to_git_arg(path)?]).map(|_| ())
}

fn path_to_git_arg(path: &std::path::Path) -> std::result::Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("worktree path is not UTF-8: {}", path.display()))
}

fn run_git(repo: &std::path::Path, args: &[&str]) -> std::result::Result<String, String> {
    let child = Command::new("git") // kanon:ignore RUST/no-direct-process-command
        .args(args)
        .current_dir(repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| format!("failed to spawn git: {source}"))?;
    let mut guard = ProcessGuard::new(child);

    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match guard.get_mut().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Err(format!("git {} timed out after 30s", args.join(" ")));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(source) => return Err(format!("git wait failed: {source}")),
        }
    };

    let mut stdout = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stdout {
        pipe.read_to_string(&mut stdout)
            .map_err(|source| format!("failed to read git stdout: {source}"))?;
    }

    let mut stderr = String::new();
    if let Some(ref mut pipe) = guard.get_mut().stderr {
        pipe.read_to_string(&mut stderr)
            .map_err(|source| format!("failed to read git stderr: {source}"))?;
    }

    #[expect(
        clippy::zombie_processes,
        reason = "process already reaped via try_wait in poll loop above"
    )]
    let _child = guard.detach();

    if stdout.len() > MAX_GIT_OUTPUT {
        let mut end = MAX_GIT_OUTPUT;
        while end > 0 && !stdout.is_char_boundary(end) {
            end -= 1;
        }
        stdout.truncate(end);
        stdout.push_str("\n[output truncated]");
    }

    if status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "git {} failed with status {}: {}",
            args.join(" "),
            status.code().unwrap_or(-1),
            stderr.trim()
        ))
    }
}

fn canonicalize_or_clone(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_under_allowed_root(path: &std::path::Path, roots: &[std::path::PathBuf]) -> bool {
    roots
        .iter()
        .map(|root| canonicalize_or_clone(root))
        .any(|root| path.starts_with(root))
}

fn format_cleanup_actions(actions: &[CleanupAction], dry_run: bool) -> String {
    if actions.is_empty() {
        return if dry_run {
            "katharos dry run: no stale worktrees found".to_owned()
        } else {
            "katharos: no stale worktrees removed".to_owned()
        };
    }

    let mut lines = Vec::with_capacity(actions.len() + 1);
    lines.push(if dry_run {
        "katharos dry run:".to_owned()
    } else {
        "katharos cleanup:".to_owned()
    });

    for action in actions {
        match action {
            CleanupAction::Removed(path) => {
                lines.push(format!("removed {}", path.display()));
            }
            CleanupAction::Pruned(path) => {
                lines.push(format!("pruned {}", path.display()));
            }
            CleanupAction::WouldRemove(path) => {
                lines.push(format!("would remove {}", path.display()));
            }
            CleanupAction::SkippedDirty(path) => {
                lines.push(format!("skipped dirty {}", path.display()));
            }
            CleanupAction::SkippedYoung(path) => {
                lines.push(format!("skipped not old enough {}", path.display()));
            }
            CleanupAction::SkippedOutsideAllowedRoot(path) => {
                lines.push(format!("skipped outside allowed roots {}", path.display()));
            }
        }
    }

    lines.join("\n")
}

// -- registration ----------------------------------------------------------

/// Register bookkeeper tools with the given registry.
///
/// # Errors
///
/// Returns an error if any tool name collides with an already-registered tool.
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    registry.register(
        tamias_def(),
        Box::new(BookkeeperStub {
            tool_name: "tamias",
        }),
    )?;
    registry.register(katharos_def(), Box::new(KatharosExecutor))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
#[allow(clippy::expect_used)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    use super::*;
    use crate::registry::ToolRegistry;

    fn result_text(result: &ToolResult) -> String {
        match &result.content {
            crate::types::ToolResultContent::Text(text) => text.clone(),
            _ => panic!("expected text content"),
        }
    }

    fn run_test_git(repo: &std::path::Path, args: &[&str]) -> String {
        run_git(repo, args).unwrap_or_else(|message| panic!("git command failed: {message}"))
    }

    fn write_test_file(path: &std::path::Path, content: &str) {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", path.display()));
        f.write_all(content.as_bytes())
            .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    }

    fn remove_test_dir(path: &std::path::Path) {
        std::fs::remove_dir_all(path)
            .unwrap_or_else(|source| panic!("failed to remove {}: {source}", path.display()));
    }

    fn test_context(
        workspace: std::path::PathBuf,
        allowed_roots: Vec<std::path::PathBuf>,
    ) -> ToolContext {
        use std::collections::HashSet;
        use std::sync::{Arc, RwLock};

        use koina::id::{NousId, SessionId};

        ToolContext {
            nous_id: NousId::new("test").expect("valid nous id"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace,
            allowed_roots,
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn init_repo(root: &std::path::Path) -> std::path::PathBuf {
        let repo = root.join("aletheia");
        std::fs::create_dir(&repo)
            .unwrap_or_else(|source| panic!("failed to create {}: {source}", repo.display()));
        run_test_git(&repo, &["init"]);
        run_test_git(&repo, &["config", "user.email", "test@example.com"]);
        run_test_git(&repo, &["config", "user.name", "Aletheia Test"]);
        write_test_file(&repo.join("README.md"), "test\n");
        run_test_git(&repo, &["add", "README.md"]);
        run_test_git(&repo, &["commit", "-m", "initial"]);
        repo
    }

    fn katharos_input(dry_run: bool) -> ToolInput {
        ToolInput {
            name: ToolName::from_static("katharos"),
            tool_use_id: "toolu_katharos".to_owned(),
            arguments: serde_json::json!({
                "project": "aletheia",
                "max_age_hours": 48,
                "dry_run": dry_run,
            }),
        }
    }

    #[test]
    fn bookkeeper_tools_register_without_collision() {
        let mut registry = ToolRegistry::new();
        register(&mut registry).expect("bookkeeper tools registered without collision");
        let defs = registry.definitions();
        assert_eq!(defs.len(), 2, "expected 2 bookkeeper tools registered");
    }

    #[test]
    fn tamias_is_system_category() {
        assert_eq!(tamias_def().category, ToolCategory::System);
    }

    #[test]
    fn katharos_is_system_category() {
        assert_eq!(katharos_def().category, ToolCategory::System);
    }

    #[test]
    fn katharos_is_irreversible() {
        assert_eq!(katharos_def().reversibility, Reversibility::Irreversible);
    }

    #[test]
    fn katharos_uses_mutating_groups() {
        let groups = katharos_def().groups;
        assert!(groups.contains(&ToolGroupId::Read));
        assert!(groups.contains(&ToolGroupId::Edit));
        assert!(groups.contains(&ToolGroupId::Command));
        assert!(groups.contains(&ToolGroupId::Verify));
    }

    #[test]
    fn tamias_is_partially_reversible() {
        assert_eq!(
            tamias_def().reversibility,
            Reversibility::PartiallyReversible
        );
    }

    #[test]
    fn no_tools_auto_activate() {
        assert!(!tamias_def().auto_activate);
        assert!(!katharos_def().auto_activate);
    }

    #[tokio::test]
    async fn stubs_return_not_implemented() {
        let ctx = test_context(
            std::path::PathBuf::from("/tmp"),
            vec![std::path::PathBuf::from("/tmp")],
        );

        let stub = BookkeeperStub {
            tool_name: "tamias",
        };
        let input = ToolInput {
            name: ToolName::from_static("tamias"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };

        let result = stub
            .execute(&input, &ctx)
            .await
            .expect("stub execute returns Ok");
        assert!(result.is_error, "stub must return an error result");
        let text = result_text(&result);
        assert!(
            text.contains("not yet implemented"),
            "error message must mention 'not yet implemented', got: {text}"
        );
    }

    #[tokio::test]
    async fn katharos_dry_run_reports_prunable_worktree_without_pruning() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(temp.path());
        let old_worktree = temp.path().join("old-worktree");
        let old_worktree_arg = old_worktree
            .to_str()
            .expect("temp paths should be UTF-8 for git");

        run_test_git(&repo, &["worktree", "add", old_worktree_arg, "-b", "old"]);
        remove_test_dir(&old_worktree);

        let ctx = test_context(repo.clone(), vec![temp.path().to_path_buf()]);
        let result = KatharosExecutor
            .execute(&katharos_input(true), &ctx)
            .await
            .expect("katharos dry-run succeeds");

        assert!(!result.is_error, "dry-run should not error");
        let text = result_text(&result);
        assert!(
            text.contains("would remove") && text.contains("old-worktree"),
            "dry-run should report prunable worktree, got: {text}"
        );
        let worktrees = run_test_git(&repo, &["worktree", "list", "--porcelain"]);
        assert!(
            worktrees.contains("old-worktree"),
            "dry-run must not prune stale metadata"
        );
    }

    #[tokio::test]
    async fn katharos_prunes_stale_worktree_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(temp.path());
        let old_worktree = temp.path().join("old-worktree");
        let old_worktree_arg = old_worktree
            .to_str()
            .expect("temp paths should be UTF-8 for git");

        run_test_git(&repo, &["worktree", "add", old_worktree_arg, "-b", "old"]);
        remove_test_dir(&old_worktree);

        let ctx = test_context(repo.clone(), vec![temp.path().to_path_buf()]);
        let result = KatharosExecutor
            .execute(&katharos_input(false), &ctx)
            .await
            .expect("katharos cleanup succeeds");

        assert!(!result.is_error, "cleanup should not error");
        let text = result_text(&result);
        assert!(
            text.contains("pruned") && text.contains("old-worktree"),
            "cleanup should report pruned metadata, got: {text}"
        );
        let worktrees = run_test_git(&repo, &["worktree", "list", "--porcelain"]);
        assert!(
            !worktrees.contains("old-worktree"),
            "cleanup should prune stale metadata"
        );
    }

    #[tokio::test]
    async fn katharos_rejects_project_mismatch() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(temp.path());
        let ctx = test_context(repo, vec![temp.path().to_path_buf()]);
        let input = ToolInput {
            name: ToolName::from_static("katharos"),
            tool_use_id: "toolu_katharos".to_owned(),
            arguments: serde_json::json!({
                "project": "other",
                "dry_run": true,
            }),
        };

        let result = KatharosExecutor
            .execute(&input, &ctx)
            .await
            .expect("project mismatch returns tool result");

        assert!(result.is_error, "project mismatch must be an error");
        let text = result_text(&result);
        assert!(
            text.contains("project mismatch"),
            "error should explain mismatch, got: {text}"
        );
    }

    #[tokio::test]
    async fn katharos_skips_worktrees_outside_allowed_roots() {
        let repo_temp = tempfile::tempdir().expect("repo tempdir");
        let outside_temp = tempfile::tempdir().expect("outside tempdir");
        let repo = init_repo(repo_temp.path());
        let outside_worktree = outside_temp.path().join("old-worktree");
        let outside_worktree_arg = outside_worktree
            .to_str()
            .expect("temp paths should be UTF-8 for git");

        run_test_git(
            &repo,
            &["worktree", "add", outside_worktree_arg, "-b", "old"],
        );
        remove_test_dir(&outside_worktree);

        let ctx = test_context(repo.clone(), vec![repo_temp.path().to_path_buf()]);
        let result = KatharosExecutor
            .execute(&katharos_input(false), &ctx)
            .await
            .expect("outside root skip succeeds");

        assert!(!result.is_error, "outside root skip should not error");
        let text = result_text(&result);
        assert!(
            text.contains("skipped outside allowed roots"),
            "cleanup should report outside-root skip, got: {text}"
        );
        let worktrees = run_test_git(&repo, &["worktree", "list", "--porcelain"]);
        assert!(
            worktrees.contains("old-worktree"),
            "outside-root stale metadata should remain for a wider allowed root"
        );
    }
}
