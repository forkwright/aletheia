//! Steward service: configurable polling loop for CI management.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::backend::StewardBackend;
use super::classify::{determine_ci_status, extract_prompt_number, extract_qa_verdict_from_body};
use super::merge::make_merge_decision;
use super::overlap::compute_merge_order;
use super::types::{
    CheckRun, CiStatus, ClassifiedPr, MergeAction, MergeOptions, MergeResult, PullRequest,
    StewardResult, SuppressionFinding,
};

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
/// Each cycle: classify PRs, make merge decisions, execute actions.
/// Respects the cancellation token for graceful shutdown.
///
/// WHY: Separating the polling loop from the single-pass logic allows
/// both daemon mode (polling) and CLI mode (single pass).
///
/// # Cancel safety
///
/// Cancel-safe at loop boundaries. The `select!` uses `cancel.cancelled()`
/// which is cancel-safe. Dropping the future between iterations simply
/// delays the next poll without losing state.
pub async fn run(
    config: &StewardConfig,
    cancel: CancellationToken,
    backend: &dyn StewardBackend,
) -> Vec<StewardResult> {
    let mut results = Vec::new();

    loop {
        tracing::info!(
            project = %config.project,
            "steward pass starting"
        );

        results.push(run_once(config, backend).await);

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

/// Classify one already-fetched pull request against its CI checks and
/// changed files.
///
/// WHY the conservative `false`/empty defaults: `blast_radius_ok`,
/// `merge_safe`, `suppression_findings`, and `has_gate_trailer` all need a
/// diff or commit-message data [`StewardBackend`] does not currently fetch
/// (only `list_open_prs`/`checks`/`files`/`merge`). Reporting them as
/// "not verified" routes every PR through `make_merge_decision`'s
/// `NeedsReview` path instead of fabricating a false "safe to auto-merge"
/// signal -- `!blast_radius_ok` alone is enough to force that path
/// regardless of tier. `prompt_number` and `qa_verdict` ARE wired for
/// real: both are pure functions of data this backend already fetches
/// (PR title and body).
fn classify_pr(pr: PullRequest, checks: &[CheckRun], config: &StewardConfig) -> ClassifiedPr {
    let ci_status = determine_ci_status(checks, &config.required_checks);
    let prompt_number = extract_prompt_number(&pr);
    let qa_verdict = extract_qa_verdict_from_body(pr.body.as_deref());

    ClassifiedPr {
        pr,
        ci_status,
        changed_files: Vec::new(),
        prompt_number,
        blast_radius_ok: false,
        merge_safe: false,
        has_gate_trailer: false,
        suppression_findings: Vec::<SuppressionFinding>::new(),
        qa_verdict,
    }
}

/// Run a single steward pass (classify, decide, act).
///
/// This is the unit of work for both polling and single-pass modes.
/// Returns the classification and action results.
///
/// # Cancel safety
///
/// Not cancel-safe: performs side effects (fetching PRs, executing merges
/// via `backend`) that are not idempotent. Do not use in `select!` branches.
pub async fn run_once(config: &StewardConfig, backend: &dyn StewardBackend) -> StewardResult {
    tracing::info!(
        project = %config.project,
        "steward single pass"
    );

    let prs = match backend.list_open_prs(&config.project).await {
        Ok(prs) => prs,
        Err(e) => {
            tracing::error!(error = %e, project = %config.project, "steward: failed to list open PRs");
            return StewardResult {
                classified: Vec::new(),
                merged: Vec::new(),
                needs_fix: Vec::new(),
                blocked: Vec::new(),
                main_ci_status: CiStatus::Unknown,
                main_fix_attempted: false,
            };
        }
    };

    let mut classified = Vec::with_capacity(prs.len());
    for pr in prs {
        let checks = match backend.checks(&config.project, &pr).await {
            Ok(checks) => checks,
            Err(e) => {
                tracing::warn!(error = %e, pr_number = pr.number, "steward: failed to fetch checks");
                Vec::new()
            }
        };
        let mut cpr = classify_pr(pr, &checks, config);
        match backend.files(&config.project, &cpr.pr).await {
            Ok(files) => cpr.changed_files = files.into_iter().map(|f| f.filename).collect(),
            Err(e) => {
                tracing::warn!(error = %e, pr_number = cpr.pr.number, "steward: failed to fetch files");
            }
        }
        classified.push(cpr);
    }

    let merge_order = compute_merge_order(&classified);
    let opts = MergeOptions {
        dry_run: config.dry_run,
        ..MergeOptions::default()
    };

    let mut merged = Vec::new();
    let mut needs_fix = Vec::new();
    let mut blocked = Vec::new();

    // WHY: merge_order groups PRs into overlap-independent batches; within
    // a batch, order doesn't matter for conflict avoidance, so a flat
    // iteration over the flattened order is sufficient here -- nothing
    // downstream currently depends on cross-batch parallelism.
    for pr_number in merge_order.into_iter().flatten() {
        let Some(cpr) = classified.iter().find(|c| c.pr.number == pr_number) else {
            continue;
        };

        if cpr.ci_status == CiStatus::Red {
            needs_fix.push(cpr.clone());
            continue;
        }

        match decide_and_act(cpr, &opts, &config.project, backend).await {
            PassOutcome::Merged(result) => merged.push(result),
            PassOutcome::Blocked(reason) => blocked.push((pr_number, reason)),
            PassOutcome::NeedsFix => needs_fix.push(cpr.clone()),
            PassOutcome::NoAction => {}
        }
    }

    StewardResult {
        classified,
        merged,
        needs_fix,
        blocked,
        main_ci_status: CiStatus::Unknown,
        main_fix_attempted: false,
    }
}

/// What happened to one classified PR after a merge decision was made and
/// (if applicable) acted on.
enum PassOutcome {
    Merged(MergeResult),
    Blocked(String),
    NeedsFix,
    NoAction,
}

/// Make and, unless `opts.dry_run`, execute a merge decision for one
/// already-classified PR.
///
/// Split out from `run_once` so it can be exercised directly against a
/// hand-built [`ClassifiedPr`] in tests -- `classify_pr`'s conservative
/// `blast_radius_ok: false` default means no `ClassifiedPr` `run_once`
/// itself can currently produce ever reaches the `Merge` branch, so
/// testing that branch through the full `run_once` path is not possible
/// today; testing this function directly is.
async fn decide_and_act(
    cpr: &ClassifiedPr,
    opts: &MergeOptions,
    project: &str,
    backend: &dyn StewardBackend,
) -> PassOutcome {
    let pr_number = cpr.pr.number;
    let decision = make_merge_decision(cpr, opts, None);
    // WHY: clone just the discriminant for matching so `decision` (a few
    // bytes -- pr_number, action, a reason String) stays whole and movable
    // into MergeResult below; matching `&decision.action` directly would
    // keep decision borrowed for arms that need to move it, and matching
    // `decision.action` by value would partial-move decision, making the
    // whole struct unusable afterward.
    match decision.action.clone() {
        MergeAction::Merge(method) => {
            if opts.dry_run {
                tracing::info!(pr_number, method = %method, "steward: dry-run, not merging");
                return PassOutcome::Merged(MergeResult {
                    pr_number,
                    decision,
                    success: false,
                    error: Some("dry_run".to_owned()),
                });
            }
            let result = backend.merge(project, &cpr.pr, method).await;
            PassOutcome::Merged(MergeResult {
                pr_number,
                decision,
                success: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            })
        }
        MergeAction::Blocked(reason) => PassOutcome::Blocked(reason),
        MergeAction::NeedsFix => PassOutcome::NeedsFix,
        MergeAction::NeedsReview | MergeAction::HoldForArchitect(_) | MergeAction::Skip(_) => {
            PassOutcome::NoAction
        }
    }
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use std::collections::HashMap;

    // WHY: parking_lot::Mutex (not tokio::sync::Mutex) — merge_calls is only
    // ever locked synchronously to record or read a call, never held across
    // an .await.
    use parking_lot::Mutex;

    use super::super::backend::{BackendError, BackendFuture};
    use super::super::types::{MergeMethod, PrFile, QaVerdictStatus};
    use super::*;

    #[derive(Default)]
    struct MockBackend {
        prs: Vec<PullRequest>,
        checks: HashMap<u64, Vec<CheckRun>>,
        files: HashMap<u64, Vec<PrFile>>,
        list_err: bool,
        merge_calls: Mutex<Vec<(u64, MergeMethod)>>,
        merge_err: bool,
    }

    impl StewardBackend for MockBackend {
        fn list_open_prs<'a>(&'a self, _project: &'a str) -> BackendFuture<'a, Vec<PullRequest>> {
            Box::pin(async move {
                if self.list_err {
                    return Err(BackendError::Request {
                        message: "list failed".to_owned(),
                    });
                }
                Ok(self.prs.clone())
            })
        }

        fn checks<'a>(
            &'a self,
            _project: &'a str,
            pr: &'a PullRequest,
        ) -> BackendFuture<'a, Vec<CheckRun>> {
            Box::pin(async move { Ok(self.checks.get(&pr.number).cloned().unwrap_or_default()) })
        }

        fn files<'a>(
            &'a self,
            _project: &'a str,
            pr: &'a PullRequest,
        ) -> BackendFuture<'a, Vec<PrFile>> {
            Box::pin(async move { Ok(self.files.get(&pr.number).cloned().unwrap_or_default()) })
        }

        fn merge<'a>(
            &'a self,
            _project: &'a str,
            pr: &'a PullRequest,
            method: MergeMethod,
        ) -> BackendFuture<'a, ()> {
            Box::pin(async move {
                self.merge_calls.lock().push((pr.number, method));
                if self.merge_err {
                    return Err(BackendError::Request {
                        message: "merge failed".to_owned(),
                    });
                }
                Ok(())
            })
        }
    }

    fn test_pr(number: u64, title: &str, body: Option<&str>) -> PullRequest {
        PullRequest {
            number,
            title: title.to_owned(),
            head_ref_name: Some(format!("branch-{number}")),
            head_sha: Some(format!("sha{number}")),
            state: Some("open".to_owned()),
            mergeable: None,
            body: body.map(str::to_owned),
            updated_at: None,
            merged_at: None,
        }
    }

    fn green_check() -> CheckRun {
        CheckRun {
            name: "build".to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("success".to_owned()),
        }
    }

    fn red_check() -> CheckRun {
        CheckRun {
            name: "build".to_owned(),
            status: "completed".to_owned(),
            conclusion: Some("failure".to_owned()),
        }
    }

    // ── StewardConfig ────────────────────────────────────────────────────

    #[test]
    fn steward_config_new_defaults() {
        let config = StewardConfig::new("acme/repo".to_string());
        assert_eq!(config.interval, Duration::from_mins(5));
        assert!(!config.once);
        assert!(!config.dry_run);
        assert_eq!(config.project, "acme/repo");
        assert!(config.required_checks.is_empty());
    }

    // ── run_once: the real wiring this issue exists to add ─────────────

    #[tokio::test]
    async fn run_once_with_no_open_prs_returns_empty_result() {
        let config = StewardConfig::new("acme/repo".to_string());
        let backend = MockBackend::default();
        let result = run_once(&config, &backend).await;
        assert!(result.classified.is_empty());
        assert!(result.merged.is_empty());
        assert_eq!(result.main_ci_status, CiStatus::Unknown);
    }

    #[tokio::test]
    async fn run_once_classifies_ci_status_from_backend_checks() {
        let config = StewardConfig::new("acme/repo".to_string());
        let pr = test_pr(1, "feat: thing", None);
        let backend = MockBackend {
            prs: vec![pr],
            checks: HashMap::from([(1, vec![green_check()])]),
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert_eq!(
            result.classified.len(),
            1,
            "the fetched PR must be classified"
        );
        assert_eq!(result.classified[0].ci_status, CiStatus::Green);
    }

    #[tokio::test]
    async fn run_once_routes_red_ci_to_needs_fix_not_merge_decision() {
        let config = StewardConfig::new("acme/repo".to_string());
        let pr = test_pr(2, "fix: thing", None);
        let backend = MockBackend {
            prs: vec![pr],
            checks: HashMap::from([(2, vec![red_check()])]),
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert_eq!(result.needs_fix.len(), 1);
        assert_eq!(result.needs_fix[0].pr.number, 2);
        assert!(result.merged.is_empty());
        assert!(
            backend.merge_calls.lock().is_empty(),
            "a red-CI PR must never reach backend.merge"
        );
    }

    #[tokio::test]
    async fn run_once_green_ci_pr_is_not_merged_without_blast_radius_data() {
        // WHY: this is the honesty contract in classify_pr's own doc comment
        // -- until a backend fetches diff data, blast_radius_ok stays false,
        // which must force NeedsReview even for a green-CI, otherwise-mergeable
        // PR rather than silently claiming it's safe to auto-merge.
        let config = StewardConfig::new("acme/repo".to_string());
        let pr = test_pr(3, "feat: safe change", Some("<!-- qa-verdict: PASS -->"));
        let backend = MockBackend {
            prs: vec![pr],
            checks: HashMap::from([(3, vec![green_check()])]),
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert!(
            result.merged.is_empty(),
            "no ClassifiedPr from run_once has blast_radius_ok=true yet"
        );
        assert!(result.needs_fix.is_empty());
        assert!(result.blocked.is_empty());
        assert!(backend.merge_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn run_once_extracts_qa_verdict_and_prompt_number_from_real_pr_data() {
        let config = StewardConfig::new("acme/repo".to_string());
        let pr = test_pr(
            4,
            "prompt-17: add feature",
            Some("<!-- qa-verdict: PARTIAL -->"),
        );
        let backend = MockBackend {
            prs: vec![pr],
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert_eq!(result.classified.len(), 1);
        assert_eq!(
            result.classified[0].qa_verdict,
            Some(QaVerdictStatus::Partial),
            "qa_verdict must come from extract_qa_verdict_from_body, not a hardcoded None"
        );
        assert_eq!(
            result.classified[0].prompt_number,
            Some(17),
            "prompt_number must come from extract_prompt_number, not a hardcoded None"
        );
    }

    #[tokio::test]
    async fn run_once_fetches_changed_files_from_backend() {
        let config = StewardConfig::new("acme/repo".to_string());
        let pr = test_pr(5, "feat: x", None);
        let backend = MockBackend {
            prs: vec![pr],
            files: HashMap::from([(
                5,
                vec![
                    PrFile {
                        filename: "src/a.rs".to_owned(),
                    },
                    PrFile {
                        filename: "src/b.rs".to_owned(),
                    },
                ],
            )]),
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert_eq!(
            result.classified[0].changed_files,
            vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()]
        );
    }

    #[tokio::test]
    async fn run_once_backend_list_failure_returns_empty_result_not_panic() {
        let config = StewardConfig::new("acme/repo".to_string());
        let backend = MockBackend {
            list_err: true,
            ..MockBackend::default()
        };
        let result = run_once(&config, &backend).await;
        assert!(result.classified.is_empty());
        assert!(result.merged.is_empty());
    }

    // ── decide_and_act: the merge-execution branch, tested directly ────
    //
    // WHY not through run_once: classify_pr's conservative blast_radius_ok
    // default means run_once itself cannot currently produce a ClassifiedPr
    // that reaches the Merge branch (see the test above). Testing
    // decide_and_act directly against a hand-built ClassifiedPr proves the
    // execution path itself is correct and ready for when a backend that
    // fetches diffs makes it reachable from run_once too.

    fn mergeable_classified_pr(number: u64) -> ClassifiedPr {
        ClassifiedPr {
            pr: test_pr(number, "feat: x", None),
            ci_status: CiStatus::Green,
            changed_files: vec!["src/a.rs".to_owned()],
            prompt_number: None,
            blast_radius_ok: true,
            merge_safe: true,
            has_gate_trailer: false,
            suppression_findings: Vec::new(),
            qa_verdict: Some(QaVerdictStatus::Pass),
        }
    }

    #[tokio::test]
    async fn decide_and_act_merges_tier1_pr_and_calls_backend() {
        let cpr = mergeable_classified_pr(10);
        let backend = MockBackend::default();
        let opts = MergeOptions::default();

        let outcome = decide_and_act(&cpr, &opts, "acme/repo", &backend).await;

        let PassOutcome::Merged(result) = outcome else {
            panic!("tier-1 PR must produce a Merged outcome");
        };
        assert!(result.success);
        assert_eq!(
            backend.merge_calls.lock().as_slice(),
            &[(10, MergeMethod::Squash)],
            "backend.merge must actually be called with the PR and configured method"
        );
    }

    #[tokio::test]
    async fn decide_and_act_dry_run_does_not_call_backend_merge() {
        let cpr = mergeable_classified_pr(11);
        let backend = MockBackend::default();
        let opts = MergeOptions {
            dry_run: true,
            ..MergeOptions::default()
        };

        let outcome = decide_and_act(&cpr, &opts, "acme/repo", &backend).await;

        let PassOutcome::Merged(result) = outcome else {
            panic!("dry-run tier-1 PR still produces a Merged-shaped (unsuccessful) outcome");
        };
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("dry_run"));
        assert!(
            backend.merge_calls.lock().is_empty(),
            "dry_run must never call backend.merge"
        );
    }

    #[tokio::test]
    async fn decide_and_act_surfaces_backend_merge_failure() {
        let cpr = mergeable_classified_pr(12);
        let backend = MockBackend {
            merge_err: true,
            ..MockBackend::default()
        };
        let opts = MergeOptions::default();

        let outcome = decide_and_act(&cpr, &opts, "acme/repo", &backend).await;

        let PassOutcome::Merged(result) = outcome else {
            panic!("expected a Merged-shaped outcome even on backend failure");
        };
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn decide_and_act_qa_fail_blocks_without_calling_backend() {
        let mut cpr = mergeable_classified_pr(13);
        cpr.qa_verdict = Some(QaVerdictStatus::Fail);
        let backend = MockBackend::default();
        let opts = MergeOptions::default();

        let outcome = decide_and_act(&cpr, &opts, "acme/repo", &backend).await;

        assert!(matches!(outcome, PassOutcome::Blocked(_)));
        assert!(backend.merge_calls.lock().is_empty());
    }

    #[tokio::test]
    async fn decide_and_act_missing_blast_radius_needs_review_not_merge() {
        let mut cpr = mergeable_classified_pr(14);
        cpr.blast_radius_ok = false;
        let backend = MockBackend::default();
        let opts = MergeOptions::default();

        let outcome = decide_and_act(&cpr, &opts, "acme/repo", &backend).await;

        assert!(matches!(outcome, PassOutcome::NoAction));
        assert!(backend.merge_calls.lock().is_empty());
    }

    // ── run(): polling loop ─────────────────────────────────────────────

    #[tokio::test]
    async fn run_single_pass_exits_immediately_and_records_one_pass() {
        let config = StewardConfig {
            once: true,
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        let backend = MockBackend::default();
        let results = run(&config, cancel, &backend).await;
        assert_eq!(
            results.len(),
            1,
            "single-pass mode must record the one run_once pass before exiting, not return empty"
        );
    }

    #[tokio::test]
    async fn run_cancellation_exits_after_one_pass() {
        let config = StewardConfig {
            interval: Duration::from_hours(1), // Long interval
            ..StewardConfig::new("acme/repo".to_string())
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let backend = MockBackend::default();
        let results = run(&config, cancel, &backend).await;
        assert_eq!(
            results.len(),
            1,
            "cancellation still records the in-flight pass before shutting down"
        );
    }
}
