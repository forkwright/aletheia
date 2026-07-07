//! Pipeline health metrics for energeia dispatch orchestration.
//!
//! Computes 7 health signals from historical dispatch and session data stored
//! in `EnergeiaStore`. Each metric has a value, status (OK/WARN/CRIT), and
//! threshold info.
//!
//! ## Metric proxies
//!
//! Several metrics require data that the store does not yet capture directly:
//!
//! | Metric | Proxy used | Missing data |
//! |--------|-----------|-------------|
//! | Corrective rate | Dispatches with Failed/Stuck sessions | QA PARTIAL/FAIL verdicts |
//! | QA false positive | PR sessions with CI failures | QA verdict data |
//! | Fix agent success | CI-validated sessions that succeeded | Fix agent marker |
//! | Observation-to-issue | `Unavailable` | Issue tracker links |
//!
//! These will improve as the store schema gains QA verdict and issue tracking
//! data.

#[cfg(feature = "storage-fjall")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "storage-fjall")]
use crate::error::{Result, StoreSnafu};
#[cfg(feature = "storage-fjall")]
use crate::store::records::{
    CiValidationRecord, CiValidationStatus, DispatchRecord, DispatchStatus, SessionRecord,
};
#[cfg(feature = "storage-fjall")]
use crate::store::{
    EnergeiaStore, SCAN_LIMIT_CI_VALIDATIONS, SCAN_LIMIT_DISPATCHES, SCAN_LIMIT_QA_VERDICTS,
    SCAN_LIMIT_SESSIONS,
};
#[cfg(feature = "storage-fjall")]
use crate::types::{QaVerdict, SessionStatus};

/// Status classification for a health metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HealthStatus {
    /// Metric is within healthy bounds.
    Ok,
    /// Metric is degraded but not critical.
    Warn,
    /// Metric indicates a critical problem requiring attention.
    Crit,
    /// Insufficient data to compute the metric (sample size zero).
    Unavailable,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warn => write!(f, "warn"),
            Self::Crit => write!(f, "crit"),
            Self::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// A single pipeline health metric with value, status, and threshold info.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HealthMetric {
    /// Short `snake_case` identifier.
    pub name: &'static str,
    /// Human-readable description of what this metric measures.
    pub description: &'static str,
    /// Computed value: rate (0.0–1.0), hours, or count depending on metric.
    pub value: f64,
    /// Status classification of the current value.
    pub status: HealthStatus,
    /// Threshold at or beyond which the status is `Ok`.
    pub ok_threshold: f64,
    /// Threshold at or beyond which the status is `Warn` (between Ok and Crit).
    pub warn_threshold: f64,
    /// Number of samples used to compute this metric (0 means unavailable).
    pub sample_size: u64,
    /// `true` when the metric uses correlated proxy data instead of direct data
    /// for the named phenomenon.
    pub is_proxied: bool,
    /// `true` if a higher value is healthier (e.g. success rate).
    pub higher_is_better: bool,
    /// Engine name label for downstream metrics export.
    pub engine_name: &'static str,
    /// Provider label for downstream metrics export.
    pub provider: &'static str,
    /// Agent identifier label for downstream metrics export.
    pub agent_id: &'static str,
}

impl HealthMetric {
    /// Whether this metric has enough data to carry a health classification.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.status != HealthStatus::Unavailable
    }

    /// Whether this metric is derived from proxy data.
    #[must_use]
    pub fn uses_proxy_data(&self) -> bool {
        self.is_proxied
    }
}

#[cfg(feature = "storage-fjall")]
const DEFAULT_ENGINE_LABEL: &str = "energeia";
#[cfg(feature = "storage-fjall")]
const DEFAULT_UNKNOWN_LABEL: &str = "unknown";

/// Aggregate pipeline health report for a time window.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HealthReport {
    /// When this report was computed.
    pub computed_at: jiff::Timestamp,
    /// Days of history included (0 = all available data).
    pub window_days: u32,
    /// All 7 pipeline health metrics.
    pub metrics: Vec<HealthMetric>,
}

impl HealthReport {
    /// Count metrics in this report that are derived from proxy data.
    #[must_use]
    pub fn proxy_metric_count(&self) -> usize {
        self.metrics.iter().filter(|m| m.is_proxied).count()
    }
}

#[cfg(feature = "storage-fjall")]
#[derive(Default)]
struct HealthAccumulator {
    /// Total dispatches inside the window.
    dispatch_total: u32,
    /// Dispatch IDs inside the window.
    dispatch_ids: HashSet<String>,
    /// Dispatch IDs with a corrective QA verdict (Partial/Fail).
    corrective_dispatch_ids: HashSet<String>,
    /// Dispatch IDs that contain at least one Failed or Stuck session.
    failed_or_stuck_dispatch_ids: HashSet<String>,
    /// `true` once any QA verdict inside the window has been seen.
    has_qa_verdicts: bool,
    /// Completed dispatches with a `finished_at` timestamp.
    completed_count: u32,
    /// Sum of cycle-time durations in milliseconds.
    completed_ms_sum: i64,
    /// Total sessions inside the window.
    session_total: u32,
    /// Sessions whose final status is `Stuck`.
    stuck_count: u32,
    /// Sessions that have a PR URL.
    sessions_with_pr: u32,
    /// Sessions with a PR URL and at least one CI validation failure.
    pr_with_ci_fail: u32,
    /// Sessions that have at least one CI validation record.
    sessions_with_ci: u32,
    /// CI-validated sessions whose final status is `Success`.
    sessions_with_ci_success: u32,
    /// Session count per dispatch for batch parallelism.
    sessions_per_dispatch: HashMap<String, u32>,
    /// Map from session ID to whether any CI validation failed.
    ci_fail_by_session: HashMap<String, bool>,
}

#[cfg(feature = "storage-fjall")]
impl HealthAccumulator {
    fn add_dispatch(&mut self, d: &DispatchRecord) {
        self.dispatch_total += 1;
        self.dispatch_ids.insert(d.id.as_str().to_owned());
        if d.status == DispatchStatus::Completed
            && let Some(finished_at) = d.finished_at
        {
            self.completed_count += 1;
            self.completed_ms_sum += finished_at.as_millisecond() - d.created_at.as_millisecond();
        }
    }

    fn add_session(&mut self, s: &SessionRecord) {
        self.session_total += 1;
        if s.status == SessionStatus::Stuck {
            self.stuck_count += 1;
        }
        if s.pr_url.is_some() {
            self.sessions_with_pr += 1;
        }
        if let Some(&has_fail) = self.ci_fail_by_session.get(s.id.as_str()) {
            self.sessions_with_ci += 1;
            if s.status == SessionStatus::Success {
                self.sessions_with_ci_success += 1;
            }
            if s.pr_url.is_some() && has_fail {
                self.pr_with_ci_fail += 1;
            }
        }
        if matches!(s.status, SessionStatus::Failed | SessionStatus::Stuck) {
            self.failed_or_stuck_dispatch_ids
                .insert(s.dispatch_id.as_str().to_owned());
        }
        *self
            .sessions_per_dispatch
            .entry(s.dispatch_id.as_str().to_owned())
            .or_insert(0) += 1;
    }

    fn add_ci_validation(&mut self, v: &CiValidationRecord) {
        let is_fail = v.status == CiValidationStatus::Fail;
        self.ci_fail_by_session
            .entry(v.session_id.as_str().to_owned())
            .and_modify(|e| *e |= is_fail)
            .or_insert(is_fail);
    }

    fn add_qa_verdict(&mut self, v: &crate::store::records::QaVerdictRecord) {
        self.has_qa_verdicts = true;
        if matches!(v.verdict, QaVerdict::Partial | QaVerdict::Fail) {
            self.corrective_dispatch_ids
                .insert(v.dispatch_id.as_str().to_owned());
        }
    }

    fn corrective_rate(&self) -> HealthMetric {
        const NAME: &str = "corrective_rate";
        const DESC: &str =
            "% of dispatches needing corrective prompts (QA Partial/Fail verdicts when available)";

        if self.dispatch_total == 0 {
            return unavailable(NAME, DESC, 0.10, 0.20, false, false);
        }

        let (corrective, is_proxied): (u32, bool) = if self.has_qa_verdicts {
            (
                u32::try_from(self.corrective_dispatch_ids.len()).unwrap_or(u32::MAX),
                false,
            )
        } else {
            (
                u32::try_from(self.failed_or_stuck_dispatch_ids.len()).unwrap_or(u32::MAX),
                true,
            )
        };

        let rate = f64::from(corrective) / f64::from(self.dispatch_total);

        HealthMetric {
            name: NAME,
            description: DESC,
            value: rate,
            status: classify_lower_is_better(rate, 0.10, 0.20),
            ok_threshold: 0.10,
            warn_threshold: 0.20,
            sample_size: u64::from(self.dispatch_total),
            is_proxied,
            higher_is_better: false,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }

    fn stuck_rate(&self) -> HealthMetric {
        const NAME: &str = "stuck_rate";
        const DESC: &str = "% of sessions ending in Stuck status (health escalation exhausted)";

        if self.session_total == 0 {
            return unavailable(NAME, DESC, 0.05, 0.15, false, false);
        }

        let rate = f64::from(self.stuck_count) / f64::from(self.session_total);

        HealthMetric {
            name: NAME,
            description: DESC,
            value: rate,
            status: classify_lower_is_better(rate, 0.05, 0.15),
            ok_threshold: 0.05,
            warn_threshold: 0.15,
            sample_size: u64::from(self.session_total),
            is_proxied: false,
            higher_is_better: false,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }

    fn qa_false_positive_rate(&self) -> HealthMetric {
        const NAME: &str = "qa_false_positive_rate";
        const DESC: &str = "% of sessions with a PR that later fail CI \
            (proxy for QA passing work that CI rejects; true rate needs QA verdict data)";

        if self.sessions_with_pr == 0 {
            return unavailable(NAME, DESC, 0.05, 0.10, false, true);
        }

        let rate = f64::from(self.pr_with_ci_fail) / f64::from(self.sessions_with_pr);

        HealthMetric {
            name: NAME,
            description: DESC,
            value: rate,
            status: classify_lower_is_better(rate, 0.05, 0.10),
            ok_threshold: 0.05,
            warn_threshold: 0.10,
            sample_size: u64::from(self.sessions_with_pr),
            is_proxied: true,
            higher_is_better: false,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }

    fn fix_agent_success_rate(&self) -> HealthMetric {
        const NAME: &str = "fix_agent_success_rate";
        const DESC: &str = "% of CI-validated sessions reaching Success \
            (proxy for fix agent success; true rate needs fix-agent marker in session data)";

        if self.sessions_with_ci == 0 {
            return unavailable(NAME, DESC, 0.80, 0.60, true, true);
        }

        let rate = f64::from(self.sessions_with_ci_success) / f64::from(self.sessions_with_ci);

        HealthMetric {
            name: NAME,
            description: DESC,
            value: rate,
            status: classify_higher_is_better(rate, 0.80, 0.60),
            ok_threshold: 0.80,
            warn_threshold: 0.60,
            sample_size: u64::from(self.sessions_with_ci),
            is_proxied: true,
            higher_is_better: true,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }

    fn cycle_time(&self) -> HealthMetric {
        const NAME: &str = "cycle_time_hours";
        const DESC: &str =
            "Average hours from dispatch creation to completion (completed dispatches only)";

        if self.completed_count == 0 {
            return unavailable(NAME, DESC, 4.0, 8.0, false, false);
        }

        let avg_hours = jiff::SignedDuration::from_millis(self.completed_ms_sum).as_secs_f64()
            / f64::from(self.completed_count)
            / 3600.0;

        HealthMetric {
            name: NAME,
            description: DESC,
            value: avg_hours,
            status: classify_lower_is_better(avg_hours, 4.0, 8.0),
            ok_threshold: 4.0,
            warn_threshold: 8.0,
            sample_size: u64::from(self.completed_count),
            is_proxied: false,
            higher_is_better: false,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }

    fn batch_parallelism(&self) -> HealthMetric {
        const NAME: &str = "batch_parallelism";
        const DESC: &str = "Average sessions per dispatch (proxy for concurrent group size)";

        if self.dispatch_total == 0 {
            return unavailable(NAME, DESC, 3.0, 1.5, true, true);
        }

        let mut total_sessions = 0u32;
        let mut with_sessions = 0u32;
        for (dispatch_id, count) in &self.sessions_per_dispatch {
            if self.dispatch_ids.contains(dispatch_id) {
                total_sessions += *count;
                with_sessions += 1;
            }
        }

        if with_sessions == 0 {
            return unavailable(NAME, DESC, 3.0, 1.5, true, true);
        }

        let avg = f64::from(total_sessions) / f64::from(with_sessions);

        HealthMetric {
            name: NAME,
            description: DESC,
            value: avg,
            status: classify_higher_is_better(avg, 3.0, 1.5),
            ok_threshold: 3.0,
            warn_threshold: 1.5,
            sample_size: u64::from(with_sessions),
            is_proxied: true,
            higher_is_better: true,
            engine_name: DEFAULT_ENGINE_LABEL,
            provider: DEFAULT_UNKNOWN_LABEL,
            agent_id: DEFAULT_UNKNOWN_LABEL,
        }
    }
}

#[cfg(feature = "storage-fjall")]
/// Compute all 7 pipeline health metrics from stored dispatch and session data.
///
/// `window_days` controls how far back to look; pass `0` to include all
/// available data. All queries are read-only.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails or if `window_days`
/// exceeds the representable [`jiff::Timestamp`] range.
pub fn compute_health_report(store: &EnergeiaStore, window_days: u32) -> Result<HealthReport> {
    let now = jiff::Timestamp::now();

    let cutoff_ms: Option<i64> = if window_days > 0 {
        let span = jiff::SignedDuration::from_hours(i64::from(window_days) * 24);
        let cutoff = now.checked_sub(span).map_err(|e| {
            StoreSnafu {
                message: format!(
                    "window_days {window_days} exceeds representable timestamp range: {e}"
                ),
            }
            .build()
        })?;
        Some(cutoff.as_millisecond())
    } else {
        None
    };

    // Stream records newest-first, stopping once timestamps fall outside the
    // requested window. Counters are updated directly, so no intermediate Vec
    // of records is allocated.
    let mut acc = HealthAccumulator::default();

    acc = store.fold_dispatches(cutoff_ms, SCAN_LIMIT_DISPATCHES, acc, |mut acc, d| {
        acc.add_dispatch(d);
        acc
    })?;

    acc = store.fold_ci_validations(cutoff_ms, SCAN_LIMIT_CI_VALIDATIONS, acc, |mut acc, v| {
        acc.add_ci_validation(v);
        acc
    })?;

    acc = store.fold_qa_verdicts(cutoff_ms, SCAN_LIMIT_QA_VERDICTS, acc, |mut acc, v| {
        acc.add_qa_verdict(v);
        acc
    })?;

    acc = store.fold_sessions(cutoff_ms, SCAN_LIMIT_SESSIONS, acc, |mut acc, s| {
        acc.add_session(s);
        acc
    })?;

    let metrics = vec![
        acc.corrective_rate(),
        acc.stuck_rate(),
        acc.qa_false_positive_rate(),
        acc.fix_agent_success_rate(),
        acc.cycle_time(),
        observation_to_issue_rate(),
        acc.batch_parallelism(),
    ];

    Ok(HealthReport {
        computed_at: now,
        window_days,
        metrics,
    })
}

/// Classify a lower-is-better rate value into OK/WARN/CRIT.
#[cfg(feature = "storage-fjall")]
fn classify_lower_is_better(value: f64, ok_threshold: f64, warn_threshold: f64) -> HealthStatus {
    if value <= ok_threshold {
        HealthStatus::Ok
    } else if value <= warn_threshold {
        HealthStatus::Warn
    } else {
        HealthStatus::Crit
    }
}

/// Classify a higher-is-better rate or count value into OK/WARN/CRIT.
#[cfg(feature = "storage-fjall")]
fn classify_higher_is_better(value: f64, ok_threshold: f64, warn_threshold: f64) -> HealthStatus {
    if value >= ok_threshold {
        HealthStatus::Ok
    } else if value >= warn_threshold {
        HealthStatus::Warn
    } else {
        HealthStatus::Crit
    }
}

/// Build an `Unavailable` metric with zeroed sample size.
#[cfg(feature = "storage-fjall")]
fn unavailable(
    name: &'static str,
    description: &'static str,
    ok_threshold: f64,
    warn_threshold: f64,
    higher_is_better: bool,
    is_proxied: bool,
) -> HealthMetric {
    HealthMetric {
        name,
        description,
        value: 0.0,
        status: HealthStatus::Unavailable,
        ok_threshold,
        warn_threshold,
        sample_size: 0,
        is_proxied,
        higher_is_better,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

// ── The 7 metrics ──

/// 1. Corrective prompt rate.
///
/// **Threshold:** <10% OK, ≤20% WARN, >20% CRIT.
///
/// **Proxy:** dispatches that contain at least one Failed or Stuck session, as
/// a fraction of all dispatches. True corrective rate requires QA PARTIAL/FAIL
/// verdict data which may not be present yet.
#[cfg(all(test, feature = "storage-fjall"))]
fn corrective_rate(
    dispatches: &[&DispatchRecord],
    sessions: &[&SessionRecord],
    qa_verdicts: &[crate::store::records::QaVerdictRecord],
) -> HealthMetric {
    const NAME: &str = "corrective_rate";
    const DESC: &str =
        "% of dispatches needing corrective prompts (QA Partial/Fail verdicts when available)";

    let total = dispatches.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.10, 0.20, false, false);
    }

    let dispatch_ids: HashSet<&str> = dispatches.iter().map(|d| d.id.as_str()).collect();
    let has_qa_verdicts = qa_verdicts
        .iter()
        .any(|v| dispatch_ids.contains(v.dispatch_id.as_str()));
    let (corrective_dispatch_ids, is_proxied): (HashSet<&str>, bool) = if has_qa_verdicts {
        (
            qa_verdicts
                .iter()
                .filter(|v| dispatch_ids.contains(v.dispatch_id.as_str()))
                .filter(|v| {
                    matches!(
                        v.verdict,
                        crate::types::QaVerdict::Partial | crate::types::QaVerdict::Fail
                    )
                })
                .map(|v| v.dispatch_id.as_str())
                .collect(),
            false,
        )
    } else {
        (
            sessions
                .iter()
                .filter(|s| matches!(s.status, SessionStatus::Failed | SessionStatus::Stuck))
                .map(|s| s.dispatch_id.as_str())
                .collect(),
            true,
        )
    };

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "dispatch counts bounded by SCAN_LIMIT_DISPATCHES (10_000), well below f64 mantissa 2^53"
    )]
    let rate = corrective_dispatch_ids.len() as f64 / total as f64; // SAFETY: counts bounded by SCAN_LIMIT_DISPATCHES (10_000)

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_DISPATCHES"
    )]
    let sample_size = total as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: rate,
        status: classify_lower_is_better(rate, 0.10, 0.20),
        ok_threshold: 0.10,
        warn_threshold: 0.20,
        sample_size,
        is_proxied,
        higher_is_better: false,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

/// 2. Stuck rate.
///
/// **Threshold:** <5% OK, ≤15% WARN, >15% CRIT.
#[cfg(all(test, feature = "storage-fjall"))]
fn stuck_rate(sessions: &[&SessionRecord]) -> HealthMetric {
    const NAME: &str = "stuck_rate";
    const DESC: &str = "% of sessions ending in Stuck status (health escalation exhausted)";

    let total = sessions.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.05, 0.15, false, false);
    }

    let stuck = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Stuck)
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts bounded by SCAN_LIMIT_SESSIONS (100_000), well below f64 mantissa 2^53"
    )]
    let rate = stuck as f64 / total as f64; // SAFETY: counts bounded by SCAN_LIMIT_SESSIONS (100_000)

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_SESSIONS"
    )]
    let sample_size = total as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: rate,
        status: classify_lower_is_better(rate, 0.05, 0.15),
        ok_threshold: 0.05,
        warn_threshold: 0.15,
        sample_size,
        is_proxied: false,
        higher_is_better: false,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

/// 3. QA false positive rate.
///
/// **Threshold:** <5% OK, ≤10% WARN, >10% CRIT.
///
/// **Proxy:** sessions with a PR URL (implying QA passed) where at least one
/// CI validation has Fail status. True rate needs persisted QA verdict data.
#[cfg(all(test, feature = "storage-fjall"))]
fn qa_false_positive_rate(
    sessions: &[&SessionRecord],
    ci_by_session: &HashMap<String, Vec<&CiValidationRecord>>,
) -> HealthMetric {
    const NAME: &str = "qa_false_positive_rate";
    const DESC: &str = "% of sessions with a PR that later fail CI \
        (proxy for QA passing work that CI rejects; true rate needs QA verdict data)";

    let sessions_with_pr: Vec<&&SessionRecord> =
        sessions.iter().filter(|s| s.pr_url.is_some()).collect();
    let total = sessions_with_pr.len();

    if total == 0 {
        return unavailable(NAME, DESC, 0.05, 0.10, false, true);
    }

    let ci_fail_count = sessions_with_pr
        .iter()
        .filter(|s| {
            ci_by_session.get(s.id.as_str()).is_some_and(|validations| {
                validations
                    .iter()
                    .any(|v| v.status == CiValidationStatus::Fail)
            })
        })
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts bounded by SCAN_LIMIT_SESSIONS (100_000), well below f64 mantissa 2^53"
    )]
    let rate = ci_fail_count as f64 / total as f64; // SAFETY: counts bounded by SCAN_LIMIT_SESSIONS (100_000)

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_SESSIONS"
    )]
    let sample_size = total as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: rate,
        status: classify_lower_is_better(rate, 0.05, 0.10),
        ok_threshold: 0.05,
        warn_threshold: 0.10,
        sample_size,
        is_proxied: true,
        higher_is_better: false,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

/// 4. Fix agent success rate.
///
/// **Threshold:** >80% OK, ≥60% WARN, <60% CRIT.
///
/// **Proxy:** among sessions that have CI validation entries (proxy for fix
/// agent sessions), the fraction that reached `Success` status. True rate
/// requires a fix-agent marker in the session data.
#[cfg(all(test, feature = "storage-fjall"))]
fn fix_agent_success_rate(
    sessions: &[&SessionRecord],
    ci_by_session: &HashMap<String, Vec<&CiValidationRecord>>,
) -> HealthMetric {
    const NAME: &str = "fix_agent_success_rate";
    const DESC: &str = "% of CI-validated sessions reaching Success \
        (proxy for fix agent success; true rate needs fix-agent marker in session data)";

    let sessions_with_ci: Vec<&&SessionRecord> = sessions
        .iter()
        .filter(|s| ci_by_session.contains_key(s.id.as_str()))
        .collect();

    let total = sessions_with_ci.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.80, 0.60, true, true);
    }

    let successes = sessions_with_ci
        .iter()
        .filter(|s| s.status == SessionStatus::Success)
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts bounded by SCAN_LIMIT_SESSIONS (100_000), well below f64 mantissa 2^53"
    )]
    let rate = successes as f64 / total as f64; // SAFETY: counts bounded by SCAN_LIMIT_SESSIONS (100_000)

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_SESSIONS"
    )]
    let sample_size = total as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: rate,
        status: classify_higher_is_better(rate, 0.80, 0.60),
        ok_threshold: 0.80,
        warn_threshold: 0.60,
        sample_size,
        is_proxied: true,
        higher_is_better: true,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

/// 5. Cycle time — average hours from dispatch creation to completion.
///
/// **Threshold:** ≤4h OK, ≤8h WARN, >8h CRIT.
#[cfg(all(test, feature = "storage-fjall"))]
fn cycle_time(dispatches: &[&DispatchRecord]) -> HealthMetric {
    const NAME: &str = "cycle_time_hours";
    const DESC: &str =
        "Average hours from dispatch creation to completion (completed dispatches only)";

    let completed: Vec<&&DispatchRecord> = dispatches
        .iter()
        .filter(|d| d.status == DispatchStatus::Completed && d.finished_at.is_some())
        .collect();

    let total = completed.len();
    if total == 0 {
        return unavailable(NAME, DESC, 4.0, 8.0, false, false);
    }

    let total_ms: i64 = completed
        .iter()
        .filter_map(|d| {
            d.finished_at
                .map(|finished| finished.as_millisecond() - d.created_at.as_millisecond())
        })
        .sum();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "total_ms sums i64 millisecond deltas across at most SCAN_LIMIT_DISPATCHES (10_000) completed dispatches; precision loss well below f64 mantissa 2^53 at practical scale"
    )]
    let avg_hours = (total_ms as f64 / total as f64) / 3_600_000.0; // SAFETY: total_ms bounded by SCAN_LIMIT_DISPATCHES deltas

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_DISPATCHES"
    )]
    let sample_size = total as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: avg_hours,
        status: classify_lower_is_better(avg_hours, 4.0, 8.0),
        ok_threshold: 4.0,
        warn_threshold: 8.0,
        sample_size,
        is_proxied: false,
        higher_is_better: false,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

/// 6. Observation-to-issue rate.
///
/// **Threshold:** >50% OK, ≥25% WARN, <25% CRIT.
///
/// Currently returns `Unavailable` — the energeia store tracks observations
/// but not issue-tracker links. This metric requires cross-system data
/// (observations × issues) not yet integrated.
#[cfg(feature = "storage-fjall")]
fn observation_to_issue_rate() -> HealthMetric {
    unavailable(
        "observation_to_issue_rate",
        "% of observations matched to tracked issues \
            (unavailable: requires issue-tracker integration not yet implemented)",
        0.50,
        0.25,
        true,
        false,
    )
}

/// 7. Batch parallelism — average sessions per dispatch.
///
/// **Threshold:** >3 OK, ≥1.5 WARN, <1.5 CRIT.
///
/// Uses total sessions per dispatch as a proxy for concurrent group size.
#[cfg(all(test, feature = "storage-fjall"))]
fn batch_parallelism(dispatches: &[&DispatchRecord], sessions: &[&SessionRecord]) -> HealthMetric {
    const NAME: &str = "batch_parallelism";
    const DESC: &str = "Average sessions per dispatch (proxy for concurrent group size)";

    let total_dispatches = dispatches.len();
    if total_dispatches == 0 {
        return unavailable(NAME, DESC, 3.0, 1.5, true, true);
    }

    let mut counts: HashMap<&str, u64> = HashMap::new();
    for s in sessions {
        *counts.entry(s.dispatch_id.as_str()).or_insert(0) += 1;
    }

    let dispatches_with_sessions: Vec<u64> = dispatches
        .iter()
        .filter_map(|d| counts.get(d.id.as_str()).copied())
        .collect();

    let n = dispatches_with_sessions.len();
    if n == 0 {
        return unavailable(NAME, DESC, 3.0, 1.5, true, true);
    }

    let total_sessions: u64 = dispatches_with_sessions.iter().sum();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "total_sessions bounded by SCAN_LIMIT_SESSIONS (100_000) and n bounded by SCAN_LIMIT_DISPATCHES (10_000); both well below f64 mantissa 2^53"
    )]
    let avg = total_sessions as f64 / n as f64; // SAFETY: totals bounded by SCAN_LIMIT_SESSIONS / SCAN_LIMIT_DISPATCHES

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: count bounded by SCAN_LIMIT_DISPATCHES"
    )]
    let sample_size = n as u64;

    HealthMetric {
        name: NAME,
        description: DESC,
        value: avg,
        status: classify_higher_is_better(avg, 3.0, 1.5),
        ok_threshold: 3.0,
        warn_threshold: 1.5,
        sample_size,
        is_proxied: true,
        higher_is_better: true,
        engine_name: DEFAULT_ENGINE_LABEL,
        provider: DEFAULT_UNKNOWN_LABEL,
        agent_id: DEFAULT_UNKNOWN_LABEL,
    }
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod health_tests;

#[cfg(test)]
#[cfg(feature = "storage-fjall")]
#[expect(
    clippy::expect_used,
    reason = "INVARIANT: test fixtures must fail fast when temp storage cannot open"
)]
mod window_tests {
    use super::*;
    use crate::store::EnergeiaStore;

    fn setup() -> (tempfile::TempDir, EnergeiaStore) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let db = fjall::Database::builder(dir.path())
            .open()
            .expect("fjall db");
        (dir, EnergeiaStore::new(&db).expect("energeia store"))
    }

    #[test]
    fn huge_window_days_returns_err_not_panic() {
        let (_dir, store) = setup();
        let result = compute_health_report(&store, u32::MAX);
        assert!(
            result.is_err(),
            "u32::MAX window_days must return Err rather than panic"
        );
    }
}
