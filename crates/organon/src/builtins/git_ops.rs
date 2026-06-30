//! Git operations tool suite: `git_status`, `git_log`, `git_diff`, `git_branch`,
//! `git_checkout`.
//!
//! WHY: Agents working inside a repository need basic Git introspection. The
//! existing `exec` tool can run git but costs tokens on parsing shell syntax
//! and does not enforce a deny-list of destructive flags. This module exposes
//! narrow, read-only operations directly.
//!
//! Scope decisions:
//! - No `git commit`, `git push`, `git reset`, `git rebase`, `git merge`:
//!   destructive or history-mutating. Agents that need these should go
//!   through the exec tool under operator review.
//! - `git_checkout` is included because switching branches is sometimes the
//!   only way to inspect state; it rejects `--force`, `-f`, and any form
//!   that would discard uncommitted changes. Creating a new branch is
//!   allowed (`create=true`) because that is non-destructive.
//!
//! Implementation: we shell out to the `git` binary via `std::process::Command`.
//! WHY: the aletheia workspace does not depend on `gix` or `git2`; pulling a
//! libgit2 binding in just for these executors would add ~4-5 MB to the
//! release binary and a C dependency. The shell-out is contained to a single
//! function that enforces the argument allowlist, so the attack surface is
//! narrow. If the workspace later adopts `gix` we can swap without changing
//! tool shapes.

use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use indexmap::IndexMap;

use koina::id::ToolName;

use crate::error::Result;
use crate::registry::{ToolExecutor, ToolRegistry};
use crate::subprocess::{SubprocessError, SubprocessRequest, SubprocessRunner};
use crate::types::{
    InputSchema, PropertyDef, PropertyType, Reversibility, ToolCategory, ToolContext, ToolDef,
    ToolDiagnostics, ToolGroupId, ToolInput, ToolResult, ToolTag,
};

use super::workspace::{extract_opt_bool, extract_opt_str, extract_opt_u64, extract_str};

/// Git subprocess wall-clock timeout.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap stdout captured from git.
const MAX_GIT_OUTPUT: usize = 256 * 1024;

/// Reject refs or arguments that start with `-` unless they are in an
/// operation-specific allowlist.
///
/// WHY: `git` treats any argument starting with `-` as an option, so an
/// attacker-controlled branch name could turn into `--upload-pack=…` and
/// execute code. Forcing callers through `--` separators is not enough when
/// we construct the command; we simply reject dashed refs outright.
fn validate_ref(raw: &str) -> std::result::Result<&str, String> {
    if raw.is_empty() {
        return Err("ref must not be empty".to_owned());
    }
    if raw.starts_with('-') {
        return Err(format!("ref must not start with '-': {raw}"));
    }
    if raw.contains('\0') || raw.contains('\n') {
        return Err("ref contains invalid characters".to_owned());
    }
    Ok(raw)
}

/// Captured output from a `git` subprocess invocation.
struct GitRunOutput {
    stdout: String,
    stderr: String,
    code: i32,
    duration_ms: u64,
}

#[derive(Clone)]
struct GitCommandRunner {
    runner: SubprocessRunner,
    program: OsString,
}

impl GitCommandRunner {
    fn new(runner: SubprocessRunner) -> Self {
        Self {
            runner,
            program: OsString::from("git"),
        }
    }
}

/// Run `git` in the workspace root. Returns stdout on success, stderr on failure.
fn run_git(
    ctx: &ToolContext,
    git: &GitCommandRunner,
    args: &[&str],
) -> std::result::Result<GitRunOutput, GitRunOutput> {
    let output = git.runner.run(
        SubprocessRequest::new(git.program.clone(), ctx.workspace.clone())
            .args(args.iter().copied())
            .timeout(GIT_TIMEOUT)
            .max_output_bytes(MAX_GIT_OUTPUT),
        ctx,
    );

    let out = match output {
        Ok(out) => out,
        Err(e) => {
            let stderr = match e {
                SubprocessError::Timeout(_) => "git timed out after 30s".to_owned(),
                other => other.to_string(),
            };
            return Err(GitRunOutput {
                stdout: String::new(),
                stderr,
                code: -1,
                duration_ms: 0,
            });
        }
    };

    let output = GitRunOutput {
        stdout: out.stdout,
        stderr: out.stderr,
        code: out.exit_code,
        duration_ms: u64::try_from(out.duration.as_millis()).unwrap_or(u64::MAX),
    };

    if output.code == 0 {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Build a [`ToolResult`] from a successful git invocation.
fn git_ok(out: GitRunOutput, empty_msg: &str) -> ToolResult {
    let content = if out.stdout.trim().is_empty() {
        empty_msg.to_owned()
    } else {
        out.stdout
    };
    ToolResult::text(content).with_diagnostics(ToolDiagnostics {
        exit_code: Some(out.code),
        stderr: if out.stderr.is_empty() {
            None
        } else {
            Some(out.stderr)
        },
        sandbox_violations: Vec::new(),
        duration_ms: out.duration_ms,
    })
}

/// Build a [`ToolResult`] from a failed git invocation.
fn git_err(out: GitRunOutput) -> ToolResult {
    let msg = format!("git exited with {}\n{}", out.code, out.stderr);
    ToolResult::error(msg).with_diagnostics(ToolDiagnostics {
        exit_code: Some(out.code),
        stderr: if out.stderr.is_empty() {
            None
        } else {
            Some(out.stderr)
        },
        sandbox_violations: Vec::new(),
        duration_ms: out.duration_ms,
    })
}

struct GitStatusExecutor {
    git: GitCommandRunner,
}

impl ToolExecutor for GitStatusExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let _ = input;
            match run_git(ctx, &self.git, &["status", "--short", "--branch"]) {
                Ok(out) => Ok(git_ok(out, "(clean working tree)")),
                Err(out) => Ok(git_err(out)),
            }
        })
    }
}

struct GitLogExecutor {
    git: GitCommandRunner,
}

impl ToolExecutor for GitLogExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let max_count = extract_opt_u64(&input.arguments, "maxCount").unwrap_or(20);
            let max_count_str = max_count.to_string();
            let mut args: Vec<&str> = vec![
                "log",
                "--pretty=format:%h %ad %an %s",
                "--date=short",
                "-n",
                &max_count_str,
            ];

            let reference = extract_opt_str(&input.arguments, "ref");
            if let Some(r) = reference {
                match validate_ref(r) {
                    Ok(valid) => args.push(valid),
                    Err(e) => return Ok(ToolResult::error(e)),
                }
            }

            match run_git(ctx, &self.git, &args) {
                Ok(out) => Ok(git_ok(out, "(no commits)")),
                Err(out) => Ok(git_err(out)),
            }
        })
    }
}

struct GitDiffExecutor {
    git: GitCommandRunner,
}

impl ToolExecutor for GitDiffExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let staged = extract_opt_bool(&input.arguments, "staged").unwrap_or(false);
            let mut args: Vec<&str> = vec!["diff"];
            if staged {
                args.push("--staged");
            }

            let reference = extract_opt_str(&input.arguments, "ref");
            if let Some(r) = reference {
                match validate_ref(r) {
                    Ok(valid) => args.push(valid),
                    Err(e) => return Ok(ToolResult::error(e)),
                }
            }

            let path = extract_opt_str(&input.arguments, "path");
            if let Some(p) = path {
                if p.starts_with('-') {
                    return Ok(ToolResult::error(format!(
                        "path must not start with '-': {p}"
                    )));
                }
                args.push("--");
                args.push(p);
            }

            match run_git(ctx, &self.git, &args) {
                Ok(out) => Ok(git_ok(out, "(no changes)")),
                Err(out) => Ok(git_err(out)),
            }
        })
    }
}

struct GitBranchExecutor {
    git: GitCommandRunner,
}

impl ToolExecutor for GitBranchExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let _ = input;
            // WHY: --list default shows local branches; --verbose adds the
            // commit subject which helps the LLM pick a branch.
            match run_git(ctx, &self.git, &["branch", "--list", "--verbose"]) {
                Ok(out) => Ok(git_ok(out, "(no local branches)")),
                Err(out) => Ok(git_err(out)),
            }
        })
    }
}

struct GitCheckoutExecutor {
    git: GitCommandRunner,
}

impl ToolExecutor for GitCheckoutExecutor {
    fn execute<'a>(
        &'a self,
        input: &'a ToolInput,
        ctx: &'a ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async {
            let branch = extract_str(&input.arguments, "branch", &input.name)?;
            let create = extract_opt_bool(&input.arguments, "create").unwrap_or(false);

            let validated = match validate_ref(branch) {
                Ok(v) => v,
                Err(e) => return Ok(ToolResult::error(e)),
            };

            // WHY: Never pass `--force` / `-f`. If the working tree is dirty,
            // let git refuse and surface the error so the agent must
            // explicitly resolve it (via commit/stash through a separate
            // path), rather than silently discarding uncommitted work.
            let args: Vec<&str> = if create {
                vec!["checkout", "-b", validated]
            } else {
                vec!["checkout", validated]
            };

            match run_git(ctx, &self.git, &args) {
                Ok(out) => Ok(git_ok(
                    out,
                    &format!(
                        "checked out {validated}{}",
                        if create { " (new branch)" } else { "" }
                    ),
                )),
                Err(out) => Ok(git_err(out)),
            }
        })
    }
}

/// Register the git tool suite.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "unit tests use default registration; runtime injects sandbox config"
    )
)]
pub(crate) fn register(registry: &mut ToolRegistry) -> Result<()> {
    register_with_sandbox(registry, crate::sandbox::SandboxConfig::default())
}

/// Register git tools with a shared subprocess sandbox config.
pub(crate) fn register_with_sandbox(
    registry: &mut ToolRegistry,
    sandbox: crate::sandbox::SandboxConfig,
) -> Result<()> {
    let git = GitCommandRunner::new(SubprocessRunner::new(sandbox));
    registry.register(
        git_status_def(),
        Box::new(GitStatusExecutor { git: git.clone() }),
    )?;
    registry.register(git_log_def(), Box::new(GitLogExecutor { git: git.clone() }))?;
    registry.register(
        git_diff_def(),
        Box::new(GitDiffExecutor { git: git.clone() }),
    )?;
    registry.register(
        git_branch_def(),
        Box::new(GitBranchExecutor { git: git.clone() }),
    )?;
    registry.register(git_checkout_def(), Box::new(GitCheckoutExecutor { git }))?;
    Ok(())
}

fn git_status_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("git_status"), // kanon:ignore RUST/expect
        description: "Show the working tree status (short format, with branch).".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn git_log_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("git_log"), // kanon:ignore RUST/expect
        description: "List recent commits in a one-line format.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "maxCount".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Number,
                        description: "Maximum commits to list (default: 20)".to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(20)),
                        ..Default::default()
                    },
                ),
                (
                    "ref".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Branch, tag, or commit to log from (default: HEAD)"
                            .to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn git_diff_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("git_diff"), // kanon:ignore RUST/expect
        description: "Show a unified diff of working-tree or staged changes.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "staged".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Show staged changes instead of working tree (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
                (
                    "ref".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Optional ref or revision range (e.g. main..HEAD)".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "path".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Limit the diff to a single path".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
            ]),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn git_branch_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("git_branch"), // kanon:ignore RUST/expect
        description: "List local branches with their latest commit.".to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::new(),
            required: vec![],
        },
        category: ToolCategory::Workspace,
        reversibility: Reversibility::FullyReversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Read],
        tags: vec![ToolTag::Recon],
    }
}

fn git_checkout_def() -> ToolDef {
    ToolDef {
        name: ToolName::from_static("git_checkout"), // kanon:ignore RUST/expect
        description:
            "Switch branches, or create and switch to a new branch. Never forces; refuses to discard uncommitted changes."
                .to_owned(),
        extended_description: None,
        input_schema: InputSchema {
            properties: IndexMap::from([
                (
                    "branch".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::String,
                        description: "Branch name to switch to".to_owned(),
                        enum_values: None,
                        default: None,
                        ..Default::default()
                    },
                ),
                (
                    "create".to_owned(),
                    PropertyDef {
                        property_type: PropertyType::Boolean,
                        description: "Create the branch if it does not exist (default: false)"
                            .to_owned(),
                        enum_values: None,
                        default: Some(serde_json::json!(false)),
                        ..Default::default()
                    },
                ),
            ]),
            required: vec!["branch".to_owned()],
        },
        category: ToolCategory::Workspace,
        // WHY: switching branches is reversible by checking out the previous
        // branch, so long as we never force-discard changes.
        reversibility: Reversibility::Reversible,
        auto_activate: true,
        groups: vec![ToolGroupId::Command],
        tags: vec![ToolTag::Edit],
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::collections::HashSet;
    use std::process::Command;
    use std::sync::{Arc, RwLock};

    use koina::id::{NousId, SessionId};

    use crate::types::ToolContext;

    use super::*;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            nous_id: NousId::new("alice").expect("valid"),
            session_id: SessionId::new(),
            turn_number: 0,
            workspace: dir.to_path_buf(),
            allowed_roots: vec![dir.to_path_buf()],
            services: None,
            active_tools: Arc::new(RwLock::new(HashSet::new())),
            tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
        }
    }

    fn init_repo(dir: &std::path::Path) {
        // Initialize a repo with a single commit so read-only ops have
        // something to report.
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "alice@acme.corp"]);
        run(&["config", "user.name", "Alice"]);
        run(&["commit", "--allow-empty", "-q", "-m", "initial"]);
    }

    fn test_sandbox() -> crate::sandbox::SandboxConfig {
        crate::sandbox::SandboxConfig {
            enabled: false,
            nproc_limit: 4096,
            ..crate::sandbox::SandboxConfig::default()
        }
    }

    fn git_runner() -> GitCommandRunner {
        GitCommandRunner::new(SubprocessRunner::new(test_sandbox()))
    }

    fn git_status_executor() -> GitStatusExecutor {
        GitStatusExecutor { git: git_runner() }
    }

    fn git_log_executor() -> GitLogExecutor {
        GitLogExecutor { git: git_runner() }
    }

    fn git_diff_executor() -> GitDiffExecutor {
        GitDiffExecutor { git: git_runner() }
    }

    fn git_branch_executor() -> GitBranchExecutor {
        GitBranchExecutor { git: git_runner() }
    }

    fn git_checkout_executor() -> GitCheckoutExecutor {
        GitCheckoutExecutor { git: git_runner() }
    }

    #[test]
    fn validate_ref_rejects_dashed_input() {
        assert!(validate_ref("--upload-pack=evil").is_err());
        assert!(validate_ref("-foo").is_err());
    }

    #[test]
    fn validate_ref_rejects_newline() {
        assert!(validate_ref("main\nHEAD").is_err());
    }

    #[test]
    fn validate_ref_accepts_normal_names() {
        assert!(validate_ref("main").is_ok());
        assert!(validate_ref("feat/some-branch").is_ok());
        assert!(validate_ref("HEAD").is_ok());
    }

    #[tokio::test]
    async fn git_status_reports_clean_tree() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_status").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };
        let result = git_status_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(!result.is_error, "git_status should succeed in a real repo");
        let text = result.content.text_summary();
        assert!(
            text.contains("main") || text.contains("clean"),
            "expected branch marker or clean tree: {text}"
        );
    }

    #[tokio::test]
    async fn git_log_reports_initial_commit() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_log").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({ "maxCount": 5 }),
        };
        let result = git_log_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(!result.is_error, "git_log should succeed");
        assert!(
            result.content.text_summary().contains("initial"),
            "log should contain the seed commit message"
        );
    }

    #[tokio::test]
    async fn git_log_rejects_dashed_ref() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_log").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({ "ref": "--upload-pack=evil" }),
        };
        let result = git_log_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(result.is_error, "dashed ref must be rejected");
    }

    #[tokio::test]
    async fn git_diff_clean_tree_is_empty() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_diff").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };
        let result = git_diff_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(!result.is_error, "git_diff should succeed");
        assert!(
            result.content.text_summary().contains("no changes"),
            "clean tree should report no changes"
        );
    }

    #[tokio::test]
    async fn git_branch_lists_main() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_branch").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };
        let result = git_branch_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(!result.is_error);
        assert!(
            result.content.text_summary().contains("main"),
            "branch listing should show main"
        );
    }

    #[tokio::test]
    async fn git_checkout_rejects_dashed_branch() {
        let dir = tempfile::tempdir().expect("tmpdir");
        init_repo(dir.path());
        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_checkout").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({ "branch": "--force" }),
        };
        let result = git_checkout_executor()
            .execute(&input, &ctx)
            .await
            .expect("exec");
        assert!(result.is_error, "dashed branch must be rejected");
    }

    #[cfg(unix)]
    #[test]
    fn git_subprocess_uses_shared_env_policy() {
        use std::os::unix::fs::PermissionsExt as _;

        let _guard = crate::subprocess::SUBPROCESS_ENV_LOCK
            .lock()
            .expect("env lock");
        let dir = tempfile::tempdir().expect("tmpdir");
        let bin_dir = tempfile::tempdir().expect("bindir");
        let fake_git = bin_dir.path().join("git");
        #[expect(
            clippy::disallowed_methods,
            reason = "test creates a fake helper binary on disk"
        )]
        std::fs::write(
            &fake_git,
            "#!/bin/sh\nprintf 'ALETHEIA_TOKEN=%s\\n' \"${ALETHEIA_TOKEN-unset}\"\n",
        )
        .expect("write fake git");
        std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake git");

        #[expect(
            unsafe_code,
            reason = "set_var requires unsafe in Rust 2024; test controls env"
        )]
        unsafe {
            std::env::set_var("ALETHEIA_TOKEN", "git-helper-secret");
        }

        let ctx = test_ctx(dir.path());
        let input = ToolInput {
            name: ToolName::new("git_status").expect("valid"),
            tool_use_id: "toolu_test".to_owned(),
            arguments: serde_json::json!({}),
        };
        let executor = GitStatusExecutor {
            git: GitCommandRunner {
                runner: SubprocessRunner::new(test_sandbox()),
                program: fake_git.into_os_string(),
            },
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let result = runtime
            .block_on(executor.execute(&input, &ctx))
            .expect("exec");

        #[expect(
            unsafe_code,
            reason = "remove_var requires unsafe in Rust 2024; test cleanup"
        )]
        unsafe {
            std::env::remove_var("ALETHEIA_TOKEN");
        }

        let text = result.content.text_summary();
        assert!(
            text.contains("ALETHEIA_TOKEN=unset"),
            "fake git should observe the sanitized child environment: {text}"
        );
        assert!(
            !text.contains("git-helper-secret"),
            "git helper subprocess must not inherit sensitive env"
        );
    }

    #[test]
    fn all_git_tools_registered() {
        let mut reg = crate::registry::ToolRegistry::new();
        register(&mut reg).expect("register");
        for name in [
            "git_status",
            "git_log",
            "git_diff",
            "git_branch",
            "git_checkout",
        ] {
            let tn = ToolName::new(name).expect("valid");
            assert!(reg.get_def(&tn).is_some(), "{name} should be registered");
        }
    }
}
