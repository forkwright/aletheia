//! Worktree isolation resolution for session execution.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use compact_str::CompactString;

use crate::prompt::WorktreePolicy;

/// Result of resolving worktree isolation for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IsolationResult {
    /// Isolation is active and the worktree is ready for use.
    Resolved {
        /// Filesystem path to the usable worktree.
        worktree: PathBuf,
    },
    /// A stale worktree entry was cleaned and the retry resolved successfully.
    StaleCleaned {
        /// Filesystem path to the usable worktree.
        worktree: PathBuf,
    },
    /// Isolation was disabled by prompt policy.
    None,
    /// Isolation could not be resolved safely.
    Blocked {
        /// Human-readable reason explaining why resolution stopped.
        reason: CompactString,
    },
}

impl IsolationResult {
    /// Stable label for metrics or structured events.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Resolved { .. } => "resolved",
            Self::StaleCleaned { .. } => "stale_cleaned",
            Self::None => "none",
            Self::Blocked { .. } => "blocked",
        }
    }

    fn worktree_path(&self) -> Option<&Path> {
        match self {
            Self::Resolved { worktree } | Self::StaleCleaned { worktree } => Some(worktree),
            Self::None | Self::Blocked { .. } => None,
        }
    }
}

/// Failure to open a registered worktree path as a usable git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorktreeOpenError {
    /// Human-readable explanation of the open failure.
    pub reason: CompactString,
}

/// Configuration for resolving a session worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct IsolationRequest {
    /// Git repository used as the source for worktree operations.
    pub repository: PathBuf,
    /// Worktree path that should back the isolated session.
    pub worktree: PathBuf,
    /// Ref checked out when the worktree must be created.
    pub base_ref: CompactString,
    /// Prompt-declared isolation policy.
    pub policy: WorktreePolicy,
}

impl IsolationRequest {
    /// Build a request with isolation enabled and `HEAD` as the base ref.
    #[must_use]
    pub fn new(repository: impl Into<PathBuf>, worktree: impl Into<PathBuf>) -> Self {
        Self {
            repository: repository.into(),
            worktree: worktree.into(),
            base_ref: CompactString::from("HEAD"),
            policy: WorktreePolicy::default(),
        }
    }
}

/// Bounded resolver for per-session worktree isolation.
#[derive(Debug, Clone)]
pub struct IsolationResolver {
    request: IsolationRequest,
}

impl IsolationResolver {
    /// Create a resolver from an isolation request.
    #[must_use]
    pub fn new(request: IsolationRequest) -> Self {
        Self { request }
    }

    /// Resolve the requested worktree, cleaning stale git metadata once.
    #[must_use]
    pub fn resolve(&self) -> IsolationResult {
        let outcome = self.resolve_inner(false, false);
        emit_outcome(&outcome);
        outcome
    }

    fn resolve_inner(&self, is_retry: bool, stale_cleaned: bool) -> IsolationResult {
        if !self.request.policy.enabled {
            return IsolationResult::None;
        }

        match self.try_resolve() {
            Ok(worktree) if stale_cleaned => IsolationResult::StaleCleaned { worktree },
            Ok(worktree) => IsolationResult::Resolved { worktree },
            Err(err) if err.is_recoverable() && !is_retry => {
                let reason = err.into_reason();
                match self.clean_stale() {
                    Ok(()) => self.resolve_inner(true, true),
                    Err(cleanup_reason) => IsolationResult::Blocked {
                        reason: CompactString::from(format!(
                            "failed to clean stale worktree after {reason}: {cleanup_reason}"
                        )),
                    },
                }
            }
            Err(err) => IsolationResult::Blocked {
                reason: err.into_reason(),
            },
        }
    }

    fn try_resolve(&self) -> std::result::Result<PathBuf, ResolveError> {
        let worktree = self.worktree_path();
        let entries = self.list_worktrees()?;

        if entries.iter().any(|entry| same_path(entry, &worktree)) {
            self.open_worktree(&worktree)?;
            return Ok(worktree);
        }

        if worktree.exists() {
            return Err(ResolveError::WorktreeListMismatch(CompactString::from(
                format!(
                    "worktree path exists but git worktree list does not include it: {}",
                    worktree.display()
                ),
            )));
        }

        self.create_worktree(&worktree)?;
        self.open_worktree(&worktree)?;
        Ok(worktree)
    }

    fn worktree_path(&self) -> PathBuf {
        if self.request.worktree.is_absolute() {
            self.request.worktree.clone()
        } else {
            self.request.repository.join(&self.request.worktree)
        }
    }

    fn list_worktrees(&self) -> std::result::Result<Vec<PathBuf>, ResolveError> {
        let output = run_git(
            &self.request.repository,
            [
                OsStr::new("worktree"),
                OsStr::new("list"),
                OsStr::new("--porcelain"),
            ],
        )
        .map_err(ResolveError::Blocked)?;
        Ok(parse_worktree_paths(&output.stdout))
    }

    fn open_worktree(&self, worktree: &Path) -> std::result::Result<(), ResolveError> {
        if !worktree.is_dir() {
            return Err(ResolveError::WorktreeOpen(WorktreeOpenError {
                reason: CompactString::from(format!(
                    "worktree is not a directory: {}",
                    worktree.display()
                )),
            }));
        }

        let output = run_git(
            worktree,
            [OsStr::new("rev-parse"), OsStr::new("--is-inside-work-tree")],
        )
        .map_err(|reason| {
            ResolveError::WorktreeOpen(WorktreeOpenError {
                reason: CompactString::from(format!(
                    "failed to open worktree {}: {reason}",
                    worktree.display()
                )),
            })
        })?;

        if output.stdout.trim() == "true" {
            Ok(())
        } else {
            Err(ResolveError::WorktreeOpen(WorktreeOpenError {
                reason: CompactString::from(format!(
                    "path is not inside a git worktree: {}",
                    worktree.display()
                )),
            }))
        }
    }

    fn create_worktree(&self, worktree: &Path) -> std::result::Result<(), ResolveError> {
        run_git(
            &self.request.repository,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                worktree.as_os_str(),
                OsStr::new(self.request.base_ref.as_str()),
            ],
        )
        .map(|_| ())
        .map_err(ResolveError::Blocked)
    }

    fn clean_stale(&self) -> std::result::Result<(), CompactString> {
        run_git(
            &self.request.repository,
            [OsStr::new("worktree"), OsStr::new("prune")],
        )
        .map(|_| ())?;

        let worktree = self.worktree_path();
        match run_git(
            &self.request.repository,
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                worktree.as_os_str(),
            ],
        ) {
            Ok(_) => Ok(()),
            Err(remove_reason) => {
                let entries = self.list_worktrees().map_err(|err| err.into_reason())?;
                if entries.iter().any(|entry| same_path(entry, &worktree)) {
                    Err(remove_reason)
                } else {
                    remove_unregistered_worktree_path(&worktree)
                }
            }
        }
    }
}

#[derive(Debug)]
enum ResolveError {
    WorktreeOpen(WorktreeOpenError),
    WorktreeListMismatch(CompactString),
    Blocked(CompactString),
}

impl ResolveError {
    fn is_recoverable(&self) -> bool {
        matches!(self, Self::WorktreeOpen(_) | Self::WorktreeListMismatch(_))
    }

    fn into_reason(self) -> CompactString {
        match self {
            Self::WorktreeOpen(error) => error.reason,
            Self::WorktreeListMismatch(reason) | Self::Blocked(reason) => reason,
        }
    }
}

struct GitOutput {
    stdout: String,
}

fn run_git<I, S>(repo: &Path, args: I) -> std::result::Result<GitOutput, CompactString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect();
    let output = Command::new("git") // kanon:ignore RUST/no-direct-process-command
        .arg("-C")
        .arg(repo)
        .args(&args)
        .output()
        .map_err(|source| CompactString::from(format!("failed to run git: {source}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        Ok(GitOutput { stdout })
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(CompactString::from(format!(
            "git {} failed: {detail}",
            display_args(&args)
        )))
    }
}

fn display_args(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy())
        .map(|arg| arg.into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_worktree_paths(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect()
}

fn remove_unregistered_worktree_path(worktree: &Path) -> std::result::Result<(), CompactString> {
    if !worktree.exists() {
        return Ok(());
    }

    // Safety rule: only remove the exact requested path after git no longer
    // lists it, and only when it is a plain directory that is empty or still
    // carries a git worktree marker. Any other contents may be operator data.
    let metadata = fs::symlink_metadata(worktree).map_err(|source| {
        CompactString::from(format!(
            "failed to inspect unregistered worktree path {}: {source}",
            worktree.display()
        ))
    })?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_dir() {
        return Err(CompactString::from(format!(
            "refusing to remove unregistered worktree path that is not a plain directory: {}",
            worktree.display()
        )));
    }

    if !is_empty_dir(worktree)? && !worktree.join(".git").exists() {
        return Err(CompactString::from(format!(
            "refusing to remove non-empty unregistered worktree path without .git marker: {}",
            worktree.display()
        )));
    }

    fs::remove_dir_all(worktree).map_err(|source| {
        CompactString::from(format!(
            "failed to remove unregistered worktree path {}: {source}",
            worktree.display()
        ))
    })
}

fn is_empty_dir(path: &Path) -> std::result::Result<bool, CompactString> {
    let mut entries = fs::read_dir(path).map_err(|source| {
        CompactString::from(format!(
            "failed to list directory {}: {source}",
            path.display()
        ))
    })?;
    Ok(entries.next().is_none())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => normalize_path(left) == normalize_path(right),
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn emit_outcome(outcome: &IsolationResult) {
    match outcome {
        IsolationResult::Blocked { reason } => {
            tracing::info!(
                target: "energeia::session::isolation",
                outcome = outcome.kind(),
                reason = %reason,
                "session isolation resolved"
            );
        }
        _ => {
            tracing::info!(
                target: "energeia::session::isolation",
                outcome = outcome.kind(),
                worktree = ?outcome.worktree_path(),
                "session isolation resolved"
            );
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn policy_disabled_returns_none_without_creating_worktree() {
        let dir = TempDir::new().unwrap();
        let repo = init_repo(dir.path());
        let worktree = dir.path().join("disabled");
        let mut request = IsolationRequest::new(&repo, &worktree);
        request.policy = WorktreePolicy { enabled: false };

        let result = IsolationResolver::new(request).resolve();

        assert_eq!(result, IsolationResult::None);
        assert!(!worktree.exists());
    }

    #[test]
    fn stale_removed_worktree_is_cleaned_and_recreated_once() {
        let dir = TempDir::new().unwrap();
        let repo = init_repo(dir.path());
        let worktree = dir.path().join("stale");
        git(
            &repo,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                worktree.as_os_str(),
                OsStr::new("HEAD"),
            ],
        );
        fs::remove_dir_all(&worktree).unwrap();
        assert!(
            listed_worktrees(&repo)
                .iter()
                .any(|path| same_path(path, &worktree))
        );

        let resolver = IsolationResolver::new(IsolationRequest::new(&repo, &worktree));
        let first = resolver.resolve();
        assert!(matches!(
            first,
            IsolationResult::StaleCleaned {
                worktree: ref resolved
            }
                if same_path(resolved, &worktree)
        ));
        assert!(worktree.join(".git").exists());

        let second = resolver.resolve();
        assert!(matches!(
            second,
            IsolationResult::Resolved {
                worktree: ref resolved
            }
                if same_path(resolved, &worktree)
        ));
    }

    #[test]
    fn retry_guard_blocks_when_recreation_fails() {
        let dir = TempDir::new().unwrap();
        let repo = init_repo(dir.path());
        let worktree = dir.path().join("blocked");
        git(
            &repo,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                worktree.as_os_str(),
                OsStr::new("HEAD"),
            ],
        );
        fs::remove_dir_all(&worktree).unwrap();

        let mut request = IsolationRequest::new(&repo, &worktree);
        request.base_ref = CompactString::from("refs/heads/missing");
        let result = IsolationResolver::new(request).resolve();

        assert!(matches!(result, IsolationResult::Blocked { .. }));
        assert!(
            !listed_worktrees(&repo)
                .iter()
                .any(|path| same_path(path, &worktree))
        );
    }

    #[test]
    fn unregistered_existing_worktree_path_is_removed_before_retry() {
        let dir = TempDir::new().unwrap();
        let repo = init_repo(dir.path());
        let worktree = dir.path().join("unregistered");
        fs::create_dir(&worktree).unwrap();
        assert!(
            !listed_worktrees(&repo)
                .iter()
                .any(|path| same_path(path, &worktree))
        );

        let resolver = IsolationResolver::new(IsolationRequest::new(&repo, &worktree));
        let first = resolver.resolve();

        assert!(matches!(
            first,
            IsolationResult::StaleCleaned {
                worktree: ref resolved
            }
                if same_path(resolved, &worktree)
        ));
        assert!(worktree.join(".git").exists());
    }

    #[test]
    fn unregistered_non_empty_path_without_git_marker_is_not_removed() {
        let dir = TempDir::new().unwrap();
        let repo = init_repo(dir.path());
        let worktree = dir.path().join("operator-data");
        fs::create_dir(&worktree).unwrap();
        fs::write(worktree.join("notes.txt"), "keep\n").unwrap();

        let resolver = IsolationResolver::new(IsolationRequest::new(&repo, &worktree));
        let result = resolver.resolve();

        assert!(matches!(result, IsolationResult::Blocked { .. }));
        assert_eq!(
            fs::read_to_string(worktree.join("notes.txt")).unwrap(),
            "keep\n"
        );
    }

    fn init_repo(parent: &Path) -> PathBuf {
        let repo = parent.join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, [OsStr::new("init")]);
        git(
            &repo,
            [OsStr::new("checkout"), OsStr::new("-b"), OsStr::new("main")],
        );
        git(
            &repo,
            [
                OsStr::new("config"),
                OsStr::new("user.name"),
                OsStr::new("alice"),
            ],
        );
        git(
            &repo,
            [
                OsStr::new("config"),
                OsStr::new("user.email"),
                OsStr::new("alice@acme.corp"),
            ],
        );
        fs::write(repo.join("README.md"), "seed\n").unwrap();
        git(&repo, [OsStr::new("add"), OsStr::new("README.md")]);
        git(
            &repo,
            [OsStr::new("commit"), OsStr::new("-m"), OsStr::new("seed")],
        );
        repo
    }

    fn listed_worktrees(repo: &Path) -> Vec<PathBuf> {
        parse_worktree_paths(
            &run_git(
                repo,
                [
                    OsStr::new("worktree"),
                    OsStr::new("list"),
                    OsStr::new("--porcelain"),
                ],
            )
            .unwrap()
            .stdout,
        )
    }

    fn git<I, S>(repo: &Path, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_git(repo, args).unwrap();
    }
}
