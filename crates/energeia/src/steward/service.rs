//! Steward service: configurable polling loop for CI management.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::backend::StewardBackend;
use super::types::{
    CiStatus, ClassifiedPr, MergeAction, MergeOptions, MergeResult, PullRequest, StewardResult,
};
use super::{classify, merge};

/// Configuration for the steward service polling loop.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StewardConfig {
    /// Polling interval between steward passes.
    pub interval: Duration,
    /// Whether to run a single pass and exit.
    pub once: bool,
    /// Dry-run mode: classify without executing actions.
    pub dry_run: bool,
    /// `GitHub` project slug (owner/repo).
    pub project: String,
    /// Required CI check names (empty = all checks matter).
    pub required_checks: Vec<String>,
}

impl StewardConfig {
    /// Create a new steward configuration with the given project slug.
    ///
    /// Defaults to a 5-minute polling interval, continuous mode (not once),
    /// and no dry-run. Use the builder-style methods to customize.
    #[must_use]
    pub fn new(project: String) -> Self {
        Self {
            interval: Duration::from_mins(5), // 5 minutes default
            once: false,
            dry_run: false,
            project,
            required_checks: Vec::new(),
        }
    }
}

/// Run the steward polling loop.
///
/// Each cycle runs one [`run_once_with_backend`] pass, then either exits
/// (`once` mode) or waits out the poll interval before the next cycle.
/// Respects the cancellation token for graceful shutdown between passes.
///
/// WHY: Separating the polling loop from the single-pass logic allows
/// both daemon mode (polling) and CLI mode (single pass).
///
/// # Cancel safety
///
/// Cancel-safe at loop boundaries only: cancellation is checked *between*
/// passes, not during one. A token already cancelled before this call still
/// lets the in-flight first pass complete -- `run_once_with_backend` itself
/// is not cancel-safe (see its own doc), so there is no safe point to
/// interrupt it mid-pass. Dropping the future between iterations simply
/// delays the next poll without losing state.
pub async fn run(
    config: &StewardConfig,
    backend: &dyn StewardBackend,
    cancel: CancellationToken,
) -> Vec<StewardResult> {
    let mut results = Vec::new();

    loop {
        tracing::info!(
            project = %config.project,
            "steward pass starting"
        );

        results.push(run_once_with_backend(config, backend).await);

        if config.once {
            tracing::info!("single-pass mode, exiting");
            break;
        }

        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                tracing::info!("steward cancelled, shutting down");
                break;
            }
            () = tokio::time::sleep(config.interval) => {}
        }
    }

    results
}

/// Run a single steward pass with no backend attached.
///
/// WHY: this is a compatibility shim preserving `run_once`'s pre-#4718
/// signature (`fn(&StewardConfig) -> StewardResult`, no backend parameter)
/// for callers that predate the [`StewardBackend`] trait --
/// `organon::builtins::energeia::steward`'s `epitropos` tool depends on this
/// exact path and arity (`energeia::steward::service::run_once`), and
/// updating that call site is outside this crate's ownership. It reproduces
/// the original placeholder behavior (empty classification, no PR fetch/CI/
/// merge I/O, `main_ci_status: CiStatus::Unknown`) by composing
/// [`run_once_with_backend`] with a backend that answers every call with
/// "nothing here," rather than duplicating the empty-result construction by
/// hand.
///
/// New callers should supply a real [`StewardBackend`] and call
/// [`run_once_with_backend`] directly -- see [`crate::backend::backend_impl::EnergeiaBackend::with_steward_backend`].
pub async fn run_once(config: &StewardConfig) -> StewardResult {
    run_once_with_backend(config, &NoopStewardBackend).await
}

/// A [`StewardBackend`] that returns no data and performs no side effects.
///
/// Used only by the [`run_once`] compatibility shim (see its WHY).
struct NoopStewardBackend;

impl StewardBackend for NoopStewardBackend {
    fn list_open_prs<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::error::Result<Vec<PullRequest>>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn check_runs<'a>(
        &'a self,
        _sha: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = crate::error::Result<Vec<super::types::CheckRun>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn changed_files<'a>(
        &'a self,
        _pr_number: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::error::Result<Vec<String>>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn diff<'a>(
        &'a self,
        _pr_number: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::Result<String>> + Send + 'a>>
    {
        Box::pin(async move { Ok(String::new()) })
    }

    fn has_gate_trailer<'a>(
        &'a self,
        _pr_number: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::Result<bool>> + Send + 'a>>
    {
        Box::pin(async move { Ok(false) })
    }

    fn declared_blast_radius<'a>(
        &'a self,
        _pr_number: u64,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = crate::error::Result<Option<Vec<String>>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(None) })
    }

    fn merge<'a>(
        &'a self,
        _pr_number: u64,
        _method: super::types::MergeMethod,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::Result<()>> + Send + 'a>>
    {
        Box::pin(async move { Ok(()) })
    }

    fn main_branch_ci_status<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::error::Result<CiStatus>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(CiStatus::Unknown) })
    }
}

/// Run a single steward pass (classify, decide, act) against `backend`.
///
/// This is the unit of work for both polling and single-pass modes:
///
/// 1. Reads the main/default branch CI status (pre-flight, informational).
/// 2. Fetches all open PRs from `backend`.
/// 3. For each PR: fetches CI check runs, changed files, diff, gate
///    trailer, and declared blast radius, and classifies it.
/// 4. PRs with `CiStatus::Red` go to `needs_fix` without a merge decision --
///    CI must go green before a merge tier is meaningful.
/// 5. PRs with `CiStatus::Pending` or `CiStatus::Unknown` are classified but
///    otherwise untouched this pass; they are re-evaluated next cycle.
/// 6. PRs with `CiStatus::Green` get a full [`merge::make_merge_decision`].
///    `MergeAction::Merge` executes through `backend.merge` (skipped in
///    `dry_run`, recorded as an unexecuted candidate instead);
///    `MergeAction::Blocked` is recorded in `blocked`. The remaining
///    actions (`HoldForArchitect`, `NeedsReview`, `Skip`) need no further
///    action this pass -- the PR stays visible via `classified`.
///
/// A backend read failure for a single PR degrades that one field (empty
/// check runs, no changed files, etc.) rather than aborting the whole pass;
/// a failure to list PRs at all aborts the pass and returns an empty result
/// alongside whatever main-branch status was read.
///
/// # Cancel safety
///
/// Not cancel-safe. This performs side effects (merges) that are not
/// idempotent. Do not use in `select!` branches.
pub async fn run_once_with_backend(
    config: &StewardConfig,
    backend: &dyn StewardBackend,
) -> StewardResult {
    tracing::info!(
        project = %config.project,
        "steward single pass"
    );

    let main_ci_status = match backend.main_branch_ci_status().await {
        Ok(status) => status,
        Err(e) => {
            tracing::warn!(
                project = %config.project,
                error = %e,
                "failed to read main branch CI status"
            );
            CiStatus::Unknown
        }
    };

    let prs = match backend.list_open_prs().await {
        Ok(prs) => prs,
        Err(e) => {
            tracing::warn!(project = %config.project, error = %e, "failed to list open PRs");
            return StewardResult {
                classified: Vec::new(),
                merged: Vec::new(),
                needs_fix: Vec::new(),
                blocked: Vec::new(),
                main_ci_status,
                main_fix_attempted: false,
            };
        }
    };

    let merge_opts = MergeOptions {
        dry_run: config.dry_run,
        ..MergeOptions::default()
    };

    let mut classified = Vec::new();
    let mut merged = Vec::new();
    let mut needs_fix = Vec::new();
    let mut blocked = Vec::new();

    for pr in prs {
        let (cp, diff_text) = classify_pr(pr, backend, config).await;
        let action = act_on_classified(&cp, &diff_text, backend, &merge_opts).await;

        if action.needs_fix {
            needs_fix.push(cp.clone());
        }
        if let Some(merge_result) = action.merge_result {
            merged.push(merge_result);
        }
        if let Some(entry) = action.blocked {
            blocked.push(entry);
        }
        classified.push(cp);
    }

    StewardResult {
        classified,
        merged,
        needs_fix,
        blocked,
        main_ci_status,
        // NOTE: repairing the main branch itself (mechanical/LLM fix pipeline
        // against a Red main_ci_status) is a distinct concern from per-PR
        // fetch/classify/merge and isn't covered by the fetch/status/merge
        // trio this backend abstracts. Tracked separately from #4718.
        main_fix_attempted: false,
    }
}

/// Fetch and classify a single PR against `backend`.
///
/// Returns the classification alongside its diff text -- the caller
/// ([`act_on_classified`]) needs the diff again for the public-API-surface
/// check, so it is fetched once here rather than twice.
async fn classify_pr(
    pr: PullRequest,
    backend: &dyn StewardBackend,
    config: &StewardConfig,
) -> (ClassifiedPr, String) {
    let pr_number = pr.number;
    let sha = pr.head_sha.clone().unwrap_or_default();

    let checks = backend.check_runs(&sha).await.unwrap_or_else(|e| {
        tracing::warn!(pr_number, error = %e, "failed to fetch check runs");
        Vec::new()
    });
    let has_gate_trailer = backend
        .has_gate_trailer(pr_number)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(pr_number, error = %e, "failed to check gate trailer");
            false
        });
    let ci_status = classify::apply_gate_trailer_override(
        classify::determine_ci_status(&checks, &config.required_checks),
        has_gate_trailer,
        pr_number,
    );

    let changed_files = backend.changed_files(pr_number).await.unwrap_or_else(|e| {
        tracing::warn!(pr_number, error = %e, "failed to fetch changed files");
        Vec::new()
    });

    let declared_blast_radius = backend
        .declared_blast_radius(pr_number)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(pr_number, error = %e, "failed to resolve declared blast radius");
            None
        });
    let blast_radius_ok = match declared_blast_radius.as_deref() {
        Some(radius) => crate::diff::all_files_within_blast_radius(&changed_files, radius),
        None => true,
    };

    let diff_text = backend.diff(pr_number).await.unwrap_or_else(|e| {
        tracing::warn!(pr_number, error = %e, "failed to fetch diff");
        String::new()
    });
    let suppression_findings = classify::parse_suppressions(&diff_text);
    // WHY: suppression findings are informational at the merge-decision
    // level (merge.rs only warns, never blocks, on `!merge_safe` when CI is
    // already green) -- CI already enforces the workspace-level lint
    // denials. A non-empty finding set still marks the PR unsafe so the
    // warning path in `make_merge_decision` fires.
    let merge_safe = suppression_findings.is_empty();
    let prompt_number = classify::extract_prompt_number(&pr);
    let qa_verdict = classify::extract_qa_verdict_from_body(pr.body.as_deref());

    let cp = ClassifiedPr {
        pr,
        ci_status,
        changed_files,
        prompt_number,
        blast_radius_ok,
        merge_safe,
        has_gate_trailer,
        suppression_findings,
        qa_verdict,
    };

    (cp, diff_text)
}

/// Bucketed outcome of acting on one already-classified PR.
struct PrAction {
    merge_result: Option<MergeResult>,
    needs_fix: bool,
    blocked: Option<(u64, String)>,
}

/// Decide and, unless dry-run, execute the action for a classified PR.
///
/// A `CiStatus::Red` PR goes straight to `needs_fix` without a merge
/// decision -- CI must go green before a merge tier is meaningful. Pending
/// or Unknown status means "re-evaluate next pass," so nothing is bucketed.
/// Only a Green PR reaches [`merge::make_merge_decision`].
async fn act_on_classified(
    cp: &ClassifiedPr,
    diff_text: &str,
    backend: &dyn StewardBackend,
    merge_opts: &MergeOptions,
) -> PrAction {
    let pr_number = cp.pr.number;

    match cp.ci_status {
        CiStatus::Red => PrAction {
            merge_result: None,
            needs_fix: true,
            blocked: None,
        },
        CiStatus::Pending | CiStatus::Unknown => PrAction {
            merge_result: None,
            needs_fix: false,
            blocked: None,
        },
        CiStatus::Green => {
            let decision = merge::make_merge_decision(cp, merge_opts, Some(diff_text));
            match decision.action {
                MergeAction::Merge(method) => {
                    let merge_result = if merge_opts.dry_run {
                        MergeResult {
                            pr_number,
                            decision,
                            success: false,
                            error: Some("dry_run: merge skipped".to_owned()),
                        }
                    } else {
                        match backend.merge(pr_number, method).await {
                            Ok(()) => MergeResult {
                                pr_number,
                                decision,
                                success: true,
                                error: None,
                            },
                            Err(e) => MergeResult {
                                pr_number,
                                decision,
                                success: false,
                                error: Some(e.to_string()),
                            },
                        }
                    };
                    PrAction {
                        merge_result: Some(merge_result),
                        needs_fix: false,
                        blocked: None,
                    }
                }
                MergeAction::Blocked(ref reason) => PrAction {
                    merge_result: None,
                    needs_fix: false,
                    blocked: Some((pr_number, reason.clone())),
                },
                MergeAction::HoldForArchitect(_)
                | MergeAction::NeedsReview
                | MergeAction::NeedsFix
                | MergeAction::Skip(_) => PrAction {
                    merge_result: None,
                    needs_fix: false,
                    blocked: None,
                },
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length collections"
)]
mod tests {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;

    use parking_lot::Mutex;

    use crate::error::Result;
    use crate::steward::types::{CheckRun, MergeMethod};

    use super::*;

    #[test]
    fn steward_config_new_defaults() {
        let config = StewardConfig::new("acme/repo".to_string());
        assert_eq!(config.interval, Duration::from_mins(5));
        assert!(!config.once);
        assert!(!config.dry_run);
        assert_eq!(config.project, "acme/repo");
        assert!(config.required_checks.is_empty());
    }

    // ── Mock backend ──

    struct MockStewardBackend {
        prs: Vec<PullRequest>,
        checks: HashMap<String, Vec<CheckRun>>,
        gate_trailers: HashMap<u64, bool>,
        changed_files: HashMap<u64, Vec<String>>,
        diffs: HashMap<u64, String>,
        blast_radius: HashMap<u64, Vec<String>>,
        main_ci_status: CiStatus,
        merge_calls: Mutex<Vec<(u64, MergeMethod)>>,
        merge_should_fail: bool,
    }

    impl MockStewardBackend {
        fn new() -> Self {
            Self {
                prs: Vec::new(),
                checks: HashMap::new(),
                gate_trailers: HashMap::new(),
                changed_files: HashMap::new(),
                diffs: HashMap::new(),
                blast_radius: HashMap::new(),
                main_ci_status: CiStatus::Green,
                merge_calls: Mutex::new(Vec::new()),
                merge_should_fail: false,
            }
        }

        fn with_pr(mut self, pr: PullRequest) -> Self {
            self.prs.push(pr);
            self
        }

        fn with_checks(mut self, sha: &str, checks: Vec<CheckRun>) -> Self {
            self.checks.insert(sha.to_owned(), checks);
            self
        }

        fn with_gate_trailer(mut self, pr_number: u64, value: bool) -> Self {
            self.gate_trailers.insert(pr_number, value);
            self
        }

        fn with_changed_files(mut self, pr_number: u64, files: Vec<String>) -> Self {
            self.changed_files.insert(pr_number, files);
            self
        }

        fn with_diff(mut self, pr_number: u64, diff: &str) -> Self {
            self.diffs.insert(pr_number, diff.to_owned());
            self
        }

        fn with_blast_radius(mut self, pr_number: u64, radius: Vec<String>) -> Self {
            self.blast_radius.insert(pr_number, radius);
            self
        }

        fn with_main_ci_status(mut self, status: CiStatus) -> Self {
            self.main_ci_status = status;
            self
        }

        fn failing_merge(mut self) -> Self {
            self.merge_should_fail = true;
            self
        }
    }

    impl StewardBackend for MockStewardBackend {
        fn list_open_prs<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<PullRequest>>> + Send + 'a>> {
            Box::pin(async move { Ok(self.prs.clone()) })
        }

        fn check_runs<'a>(
            &'a self,
            sha: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<CheckRun>>> + Send + 'a>> {
            Box::pin(async move { Ok(self.checks.get(sha).cloned().unwrap_or_default()) })
        }

        fn changed_files<'a>(
            &'a self,
            pr_number: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
            Box::pin(async move {
                Ok(self
                    .changed_files
                    .get(&pr_number)
                    .cloned()
                    .unwrap_or_default())
            })
        }

        fn diff<'a>(
            &'a self,
            pr_number: u64,
        ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
            Box::pin(async move { Ok(self.diffs.get(&pr_number).cloned().unwrap_or_default()) })
        }

        fn has_gate_trailer<'a>(
            &'a self,
            pr_number: u64,
        ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
            Box::pin(async move { Ok(self.gate_trailers.get(&pr_number).copied().unwrap_or(false)) })
        }

        fn declared_blast_radius<'a>(
            &'a self,
            pr_number: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<String>>>> + Send + 'a>> {
            Box::pin(async move { Ok(self.blast_radius.get(&pr_number).cloned()) })
        }

        fn merge<'a>(
            &'a self,
            pr_number: u64,
            method: MergeMethod,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
            Box::pin(async move {
                self.merge_calls.lock().push((pr_number, method));
                if self.merge_should_fail {
                    return Err(crate::error::EngineSnafu {
                        detail: "mock merge failure",
                    }
                    .build());
                }
                Ok(())
            })
        }

        fn main_branch_ci_status<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<CiStatus>> + Send + 'a>> {
            Box::pin(async move { Ok(self.main_ci_status) })
        }
    }

    fn green_check(name: &str) -> CheckRun {
        CheckRun {
            name: name.to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
        }
    }

    fn red_check(name: &str) -> CheckRun {
        CheckRun {
            name: name.to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("failure".to_owned()),
        }
    }

    fn make_pr(number: u64, head_sha: &str, mergeable: &str, body: Option<&str>) -> PullRequest {
        PullRequest {
            number,
            title: format!("PR #{number}"),
            head_ref_name: Some(format!("branch-{number}")),
            head_sha: Some(head_sha.to_owned()),
            state: Some("open".to_owned()),
            mergeable: Some(mergeable.to_owned()),
            body: body.map(str::to_owned),
            updated_at: None,
            merged_at: None,
        }
    }

    // ── run_once compatibility shim (no backend) ──

    #[tokio::test]
    async fn run_once_without_backend_reproduces_original_placeholder_shape() {
        // WHY(#4718): organon::builtins::energeia::steward calls
        // `energeia::steward::service::run_once(&config)` directly, at that
        // exact path and arity, and cannot be updated from this crate. This
        // pins the compatibility shim's output to the pre-#4718 placeholder
        // shape so a future change to run_once_with_backend or
        // NoopStewardBackend can't silently break that external contract.
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once(&config).await;

        assert!(result.classified.is_empty());
        assert!(result.merged.is_empty());
        assert!(result.needs_fix.is_empty());
        assert!(result.blocked.is_empty());
        assert_eq!(result.main_ci_status, CiStatus::Unknown);
        assert!(!result.main_fix_attempted);
    }

    // ── run_once_with_backend ──

    #[tokio::test]
    async fn run_once_with_no_open_prs_returns_empty_result() {
        let backend = MockStewardBackend::new();
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert!(result.classified.is_empty());
        assert!(result.merged.is_empty());
        assert!(result.needs_fix.is_empty());
        assert!(result.blocked.is_empty());
        assert_eq!(result.main_ci_status, CiStatus::Green);
    }

    #[tokio::test]
    async fn run_once_propagates_main_branch_ci_status() {
        let backend = MockStewardBackend::new().with_main_ci_status(CiStatus::Red);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.main_ci_status, CiStatus::Red);
    }

    #[tokio::test]
    async fn run_once_auto_merges_tier1_green_pr() {
        let pr = make_pr(1, "sha1", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha1", vec![green_check("build")])
            .with_changed_files(1, vec!["crates/energeia/src/lib.rs".to_owned()]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.merged.len(), 1);
        assert!(result.merged[0].success);
        assert!(matches!(
            result.merged[0].decision.action,
            MergeAction::Merge(MergeMethod::Squash)
        ));
        assert_eq!(
            *backend.merge_calls.lock(),
            vec![(1, MergeMethod::Squash)]
        );
    }

    #[tokio::test]
    async fn run_once_dry_run_records_candidate_without_executing_merge() {
        let pr = make_pr(1, "sha1", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha1", vec![green_check("build")])
            .with_changed_files(1, vec!["crates/energeia/src/lib.rs".to_owned()]);
        let config = StewardConfig {
            dry_run: true,
            ..StewardConfig::new("acme/repo".to_owned())
        };

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.merged.len(), 1);
        assert!(!result.merged[0].success);
        assert!(
            result.merged[0]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("dry_run"))
        );
        assert!(
            backend.merge_calls.lock().is_empty(),
            "dry_run must not execute the merge"
        );
    }

    #[tokio::test]
    async fn run_once_merge_failure_is_recorded_not_panicked() {
        let pr = make_pr(1, "sha1", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha1", vec![green_check("build")])
            .with_changed_files(1, vec!["crates/energeia/src/lib.rs".to_owned()])
            .failing_merge();
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.merged.len(), 1);
        assert!(!result.merged[0].success);
        assert!(result.merged[0].error.is_some());
    }

    #[tokio::test]
    async fn run_once_routes_red_ci_pr_to_needs_fix() {
        let pr = make_pr(2, "sha2", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha2", vec![red_check("build")]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.needs_fix.len(), 1);
        assert_eq!(result.needs_fix[0].pr.number, 2);
        assert!(result.merged.is_empty());
        assert!(
            backend.merge_calls.lock().is_empty(),
            "a red PR must never reach a merge decision"
        );
    }

    #[tokio::test]
    async fn run_once_blocks_pr_with_merge_conflict() {
        let pr = make_pr(3, "sha3", "CONFLICTING", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha3", vec![green_check("build")]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.blocked, vec![(3, "merge conflict".to_owned())]);
        assert!(result.merged.is_empty());
    }

    #[tokio::test]
    async fn run_once_holds_pr_with_missing_qa_verdict() {
        let pr = make_pr(4, "sha4", "MERGEABLE", None);
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha4", vec![green_check("build")]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert!(result.merged.is_empty());
        assert!(result.blocked.is_empty());
        assert!(result.needs_fix.is_empty());
        assert!(result.classified.iter().any(|c| c.pr.number == 4));
    }

    #[tokio::test]
    async fn run_once_declared_blast_radius_violation_skips_merge() {
        let pr = make_pr(5, "sha5", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha5", vec![green_check("build")])
            .with_changed_files(5, vec!["crates/other/src/lib.rs".to_owned()])
            .with_blast_radius(5, vec!["crates/energeia/".to_owned()]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert!(result.merged.is_empty());
        assert!(
            result
                .classified
                .iter()
                .any(|c| c.pr.number == 5 && !c.blast_radius_ok)
        );
    }

    #[tokio::test]
    async fn run_once_holds_pr_with_public_api_change() {
        let pr = make_pr(6, "sha6", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_checks("sha6", vec![green_check("build")])
            .with_changed_files(6, vec!["crates/energeia/src/lib.rs".to_owned()])
            .with_diff(
                6,
                "+++ b/crates/energeia/src/lib.rs\n+pub fn new_public_api() {}\n",
            );
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert!(
            result.merged.is_empty(),
            "public API surface change must hold for architect review, not auto-merge"
        );
        assert!(result.blocked.is_empty());
        assert!(result.classified.iter().any(|c| c.pr.number == 6));
    }

    #[tokio::test]
    async fn run_once_applies_gate_trailer_override_when_ci_checks_absent() {
        // WHY: apply_gate_trailer_override upgrades a status to Green only
        // when it was NOT already Green; determine_ci_status returns Unknown
        // for an empty check list, so this exercises the override path
        // rather than a no-op.
        let pr = make_pr(6, "sha6", "MERGEABLE", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockStewardBackend::new()
            .with_pr(pr)
            .with_gate_trailer(6, true)
            .with_changed_files(6, vec!["crates/energeia/src/lib.rs".to_owned()]);
        let config = StewardConfig::new("acme/repo".to_owned());

        let result = run_once_with_backend(&config, &backend).await;

        assert_eq!(result.merged.len(), 1);
        assert!(result.merged[0].success);
    }

    // ── run (polling loop) ──

    #[tokio::test]
    async fn run_single_pass_executes_exactly_one_pass_then_exits() {
        let backend = MockStewardBackend::new();
        let config = StewardConfig {
            once: true,
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        let results = run(&config, &backend, cancel).await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn run_cancellation_exits_after_the_in_flight_pass() {
        let backend = MockStewardBackend::new();
        let config = StewardConfig {
            interval: Duration::from_hours(1), // Long interval
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let results = run(&config, &backend, cancel).await;
        // WHY: cancellation is checked at loop *boundaries*, between passes --
        // a token already cancelled before the call still lets the first
        // pass (already in flight when the boundary check runs) complete.
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn run_polls_multiple_passes_until_cancelled() {
        let backend = MockStewardBackend::new();
        let config = StewardConfig {
            interval: Duration::from_millis(1),
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel_clone.cancel();
        });
        let results = run(&config, &backend, cancel).await;
        assert!(
            results.len() >= 2,
            "expected multiple polling passes, got {}",
            results.len()
        );
    }
}
