//! External-interaction boundary for the steward pipeline.
//!
//! [`StewardBackend`] is the trait `mod.rs`'s own doc comment already
//! promised ("external interactions ... are abstracted behind backend
//! traits that callers provide") but that never existed until now --
//! `run_once` returned a hardcoded empty result because nothing ever
//! fetched a PR to feed the pure classify/merge/overlap/fix functions.

use std::future::Future;
use std::pin::Pin;

use snafu::Snafu;

use super::types::{CheckRun, MergeMethod, PrFile, PullRequest};

/// Boxed future returned by every [`StewardBackend`] method.
///
/// Mirrors `organon::registry::ToolExecutor`'s object-safe async-trait
/// pattern (this workspace's existing idiom for a `dyn`-dispatchable async
/// trait) rather than pulling in `async-trait`, which nothing here
/// currently depends on.
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

/// Failure from a [`StewardBackend`] call.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum BackendError {
    /// The underlying transport (HTTP, git subprocess, ...) failed.
    #[snafu(display("steward backend request failed: {message}"))]
    Request {
        /// Human-readable failure detail.
        message: String,
    },
}

/// External interactions the steward pipeline needs, abstracted so the
/// pure classify/merge/overlap/fix/conflict functions in this module never
/// touch a network socket or a subprocess directly.
///
/// `project` is the `owner/repo` slug ([`super::service::StewardConfig::project`]).
/// `checks`/`files`/`merge` take the already-fetched [`PullRequest`] rather
/// than a bare PR number so an implementation has `head_sha` on hand
/// without a second round trip to re-fetch it.
pub trait StewardBackend: Send + Sync {
    /// List open pull requests for `project`.
    fn list_open_prs<'a>(&'a self, project: &'a str) -> BackendFuture<'a, Vec<PullRequest>>;

    /// Fetch CI check runs for `pr`'s head commit.
    fn checks<'a>(
        &'a self,
        project: &'a str,
        pr: &'a PullRequest,
    ) -> BackendFuture<'a, Vec<CheckRun>>;

    /// Fetch the list of files `pr` changes.
    fn files<'a>(&'a self, project: &'a str, pr: &'a PullRequest)
    -> BackendFuture<'a, Vec<PrFile>>;

    /// Merge `pr` using `method`.
    fn merge<'a>(
        &'a self,
        project: &'a str,
        pr: &'a PullRequest,
        method: MergeMethod,
    ) -> BackendFuture<'a, ()>;
}
