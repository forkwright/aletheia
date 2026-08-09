//! Steward backend trait: the boundary between the pure classify/merge/fix
//! decision logic in this module and the external systems (`GitHub` API,
//! git subprocess) a steward pass acts on.
//!
//! WHY(#4718): the steward pipeline's decision functions (`classify`,
//! `merge`, `conflict`, `fix`, `overlap`) are pure and unit-tested against
//! hand-built fixtures. Fetching PRs, reading CI status, and executing
//! merges are the one part of a steward pass that talks to the outside
//! world; putting that behind a trait keeps the decision logic testable
//! without a live `GitHub` connection and lets callers (kanon, a daemon)
//! supply whatever transport they already have (REST client, `gh` CLI
//! subprocess, a git worktree) without this crate depending on any of them.

use std::future::Future;
use std::pin::Pin;

use crate::error::Result;

use super::types::{CheckRun, CiStatus, MergeMethod, PullRequest};

/// External interactions a steward pass needs: fetching PRs, reading CI
/// status, and executing merges.
///
/// Implementations live outside this crate (see the module-level WHY).
pub trait StewardBackend: Send + Sync {
    /// Fetch all open pull requests for the configured project.
    fn list_open_prs<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<PullRequest>>> + Send + 'a>>;

    /// Fetch CI check runs for the given commit SHA.
    fn check_runs<'a>(
        &'a self,
        sha: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CheckRun>>> + Send + 'a>>;

    /// Fetch the list of files changed by a pull request.
    fn changed_files<'a>(
        &'a self,
        pr_number: u64,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

    /// Fetch the unified diff for a pull request.
    fn diff<'a>(&'a self, pr_number: u64)
    -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

    /// Whether any commit on the pull request carries a `Gate-Passed` trailer.
    ///
    /// WHY: the trailer proves the local gate passed without depending on
    /// `GitHub` CI (see [`super::classify::apply_gate_trailer_override`]).
    /// Reading commit trailers requires either a git subprocess or the
    /// commits API, which is backend-specific.
    fn has_gate_trailer<'a>(
        &'a self,
        pr_number: u64,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;

    /// The declared blast radius for the prompt this PR implements, if the
    /// backend can resolve one (e.g. from a prompt queue keyed by the
    /// number extracted from the PR title/branch).
    ///
    /// `None` means "no declared scope is known to this backend" and is
    /// treated the same as an empty blast radius: unrestricted.
    fn declared_blast_radius<'a>(
        &'a self,
        pr_number: u64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<String>>>> + Send + 'a>>;

    /// Execute a merge for a pull request using the given method.
    fn merge<'a>(
        &'a self,
        pr_number: u64,
        method: MergeMethod,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    /// Aggregate CI status of the default/main branch (pre-flight check).
    fn main_branch_ci_status<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<CiStatus>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // WHY: compile-time check that StewardBackend is object-safe, mirroring
    // crate::backend::DispatchBackend's own check -- `run_once` takes
    // `&dyn StewardBackend` so callers can supply any implementation at
    // runtime without this crate depending on a concrete transport.
    const _: Option<&dyn StewardBackend> = None;
}
