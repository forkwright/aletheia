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
//! | QA false positive | PR sessions with CI failures | QA verdict records |
//! | Fix agent success | CI-validated sessions that succeeded | Fix agent marker |
//! | Observation-to-issue | `Unavailable` | Issue tracker records |
//!
//! These will improve as the store schema is extended with QA verdict and issue
//! tracking records.

#[cfg(feature = "storage-fjall")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "storage-fjall")]
use crate::error::Result;
#[cfg(feature = "storage-fjall")]
use crate::store::records::{
    CiValidationRecord, CiValidationStatus, DispatchRecord, DispatchStatus, SessionRecord,
};
#[cfg(feature = "storage-fjall")]
use crate::store::{
    EnergeiaStore, SCAN_LIMIT_CI_VALIDATIONS, SCAN_LIMIT_DISPATCHES, SCAN_LIMIT_SESSIONS,
};
#[cfg(feature = "storage-fjall")]
use crate::types::SessionStatus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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
    /// Number of records used to compute this metric (0 means unavailable).
    pub sample_size: u64,
    /// `true` if a higher value is healthier (e.g. success rate).
    pub higher_is_better: bool,
}

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(feature = "storage-fjall")]
/// Compute all 7 pipeline health metrics from stored dispatch and session data.
///
/// `window_days` controls how far back to look; pass `0` to include all
/// available data. All queries are read-only.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails.
pub fn compute_health_report(store: &EnergeiaStore, window_days: u32) -> Result<HealthReport> {
    let now = jiff::Timestamp::now();

    let cutoff_ms: Option<i64> = if window_days > 0 {
        let span = jiff::SignedDuration::from_hours(i64::from(window_days) * 24);
        #[expect(
            clippy::expect_used,
            reason = "bounded subtraction from now is infallible for realistic day counts"
        )]
        let cutoff = now
            .checked_sub(span)
            .expect("timestamp subtraction within realistic day range");
        Some(cutoff.as_millisecond())
    } else {
        None
    };

    // Load raw data — time filtering happens in-process after the scans.
    let all_dispatches = store.list_dispatches(SCAN_LIMIT_DISPATCHES)?;
    let all_sessions = store.list_all_sessions(SCAN_LIMIT_SESSIONS)?;
    let all_ci_validations = store.list_all_ci_validations(SCAN_LIMIT_CI_VALIDATIONS)?;

    let dispatches: Vec<&DispatchRecord> = all_dispatches
        .iter()
        .filter(|d| cutoff_ms.is_none_or(|cutoff| d.created_at.as_millisecond() >= cutoff))
        .collect();

    let sessions: Vec<&SessionRecord> = all_sessions
        .iter()
        .filter(|s| cutoff_ms.is_none_or(|cutoff| s.created_at.as_millisecond() >= cutoff))
        .collect();

    // Build session-id → CI validations map for O(1) per-session lookup.
    let ci_by_session: HashMap<String, Vec<&CiValidationRecord>> = {
        let mut map: HashMap<String, Vec<&CiValidationRecord>> = HashMap::new();
        for v in &all_ci_validations {
            map.entry(v.session_id.as_str().to_owned())
                .or_default()
                .push(v);
        }
        map
    };

    let metrics = vec![
        corrective_rate(&dispatches, &sessions),
        stuck_rate(&sessions),
        qa_false_positive_rate(&sessions, &ci_by_session),
        fix_agent_success_rate(&sessions, &ci_by_session),
        cycle_time(&dispatches),
        observation_to_issue_rate(),
        batch_parallelism(&dispatches, &sessions),
    ];

    Ok(HealthReport {
        computed_at: now,
        window_days,
        metrics,
    })
}

// ---------------------------------------------------------------------------
// Metric helpers
// ---------------------------------------------------------------------------

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
) -> HealthMetric {
    HealthMetric {
        name,
        description,
        value: 0.0,
        status: HealthStatus::Unavailable,
        ok_threshold,
        warn_threshold,
        sample_size: 0,
        higher_is_better,
    }
}

// ---------------------------------------------------------------------------
// The 7 metrics
// ---------------------------------------------------------------------------

/// 1. Corrective prompt rate.
///
/// **Threshold:** <10% OK, ≤20% WARN, >20% CRIT.
///
/// **Proxy:** dispatches that contain at least one Failed or Stuck session, as
/// a fraction of all dispatches. True corrective rate requires QA PARTIAL/FAIL
/// verdict records which are not yet stored.
#[cfg(feature = "storage-fjall")]
fn corrective_rate(dispatches: &[&DispatchRecord], sessions: &[&SessionRecord]) -> HealthMetric {
    const NAME: &str = "corrective_rate";
    const DESC: &str = "% of dispatches needing corrective prompts \
        (proxy: dispatches with Failed/Stuck sessions; true rate needs QA verdicts)";

    let total = dispatches.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.10, 0.20, false);
    }

    let corrective_dispatch_ids: HashSet<&str> = sessions
        .iter()
        .filter(|s| matches!(s.status, SessionStatus::Failed | SessionStatus::Stuck))
        .map(|s| s.dispatch_id.as_str())
        .collect();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "dispatch counts are bounded; precision loss unreachable at practical scale"
    )]
    let rate = corrective_dispatch_ids.len() as f64 / total as f64;

    #[expect(
        clippy::as_conversions,
        reason = "dispatch count is bounded by SCAN_LIMIT_DISPATCHES, fits u64"
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
        higher_is_better: false,
    }
}

/// 2. Stuck rate.
///
/// **Threshold:** <5% OK, ≤15% WARN, >15% CRIT.
#[cfg(feature = "storage-fjall")]
fn stuck_rate(sessions: &[&SessionRecord]) -> HealthMetric {
    const NAME: &str = "stuck_rate";
    const DESC: &str = "% of sessions ending in Stuck status (health escalation exhausted)";

    let total = sessions.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.05, 0.15, false);
    }

    let stuck = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Stuck)
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts are bounded; precision loss unreachable at practical scale"
    )]
    let rate = stuck as f64 / total as f64;

    #[expect(
        clippy::as_conversions,
        reason = "session count fits u64 within SCAN_LIMIT_SESSIONS"
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
        higher_is_better: false,
    }
}

/// 3. QA false positive rate.
///
/// **Threshold:** <5% OK, ≤10% WARN, >10% CRIT.
///
/// **Proxy:** sessions with a PR URL (implying QA passed) where at least one
/// CI validation record has Fail status. True rate needs stored QA verdicts.
#[cfg(feature = "storage-fjall")]
fn qa_false_positive_rate(
    sessions: &[&SessionRecord],
    ci_by_session: &HashMap<String, Vec<&CiValidationRecord>>,
) -> HealthMetric {
    const NAME: &str = "qa_false_positive_rate";
    const DESC: &str = "% of sessions with a PR that later fail CI \
        (proxy for QA passing work that CI rejects; true rate needs QA verdict records)";

    let sessions_with_pr: Vec<&&SessionRecord> =
        sessions.iter().filter(|s| s.pr_url.is_some()).collect();
    let total = sessions_with_pr.len();

    if total == 0 {
        return unavailable(NAME, DESC, 0.05, 0.10, false);
    }

    let ci_fail_count = sessions_with_pr
        .iter()
        .filter(|s| {
            ci_by_session
                .get(s.id.as_str())
                .is_some_and(|validations| {
                    validations
                        .iter()
                        .any(|v| v.status == CiValidationStatus::Fail)
                })
        })
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts are bounded; precision loss unreachable at practical scale"
    )]
    let rate = ci_fail_count as f64 / total as f64;

    #[expect(
        clippy::as_conversions,
        reason = "session count fits u64 within SCAN_LIMIT_SESSIONS"
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
        higher_is_better: false,
    }
}

/// 4. Fix agent success rate.
///
/// **Threshold:** >80% OK, ≥60% WARN, <60% CRIT.
///
/// **Proxy:** among sessions that have CI validation records (proxy for fix
/// agent sessions), the fraction that reached `Success` status. True rate
/// requires a fix-agent marker in the session record.
#[cfg(feature = "storage-fjall")]
fn fix_agent_success_rate(
    sessions: &[&SessionRecord],
    ci_by_session: &HashMap<String, Vec<&CiValidationRecord>>,
) -> HealthMetric {
    const NAME: &str = "fix_agent_success_rate";
    const DESC: &str = "% of CI-validated sessions reaching Success \
        (proxy for fix agent success; true rate needs fix-agent marker in session record)";

    let sessions_with_ci: Vec<&&SessionRecord> = sessions
        .iter()
        .filter(|s| ci_by_session.contains_key(s.id.as_str()))
        .collect();

    let total = sessions_with_ci.len();
    if total == 0 {
        return unavailable(NAME, DESC, 0.80, 0.60, true);
    }

    let successes = sessions_with_ci
        .iter()
        .filter(|s| s.status == SessionStatus::Success)
        .count();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "session counts are bounded; precision loss unreachable at practical scale"
    )]
    let rate = successes as f64 / total as f64;

    #[expect(
        clippy::as_conversions,
        reason = "session count fits u64 within SCAN_LIMIT_SESSIONS"
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
        higher_is_better: true,
    }
}

/// 5. Cycle time — average hours from dispatch creation to completion.
///
/// **Threshold:** ≤4h OK, ≤8h WARN, >8h CRIT.
#[cfg(feature = "storage-fjall")]
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
        return unavailable(NAME, DESC, 4.0, 8.0, false);
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
        reason = "total_ms as f64: bounded by dispatch count × max session duration; precision loss unreachable"
    )]
    let avg_hours = (total_ms as f64 / total as f64) / 3_600_000.0;

    #[expect(
        clippy::as_conversions,
        reason = "dispatch count fits u64 within SCAN_LIMIT_DISPATCHES"
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
        higher_is_better: false,
    }
}

/// 6. Observation-to-issue rate.
///
/// **Threshold:** >50% OK, ≥25% WARN, <25% CRIT.
///
/// Currently returns `Unavailable` — the energeia store tracks observations
/// but not issue-tracker records. This metric requires cross-system data
/// (observation records × issue tracker entries) not yet integrated.
#[cfg(feature = "storage-fjall")]
fn observation_to_issue_rate() -> HealthMetric {
    unavailable(
        "observation_to_issue_rate",
        "% of observations matched to tracked issues \
            (unavailable: requires issue-tracker integration not yet implemented)",
        0.50,
        0.25,
        true,
    )
}

/// 7. Batch parallelism — average sessions per dispatch.
///
/// **Threshold:** >3 OK, ≥1.5 WARN, <1.5 CRIT.
///
/// Uses total sessions per dispatch as a proxy for concurrent group size.
#[cfg(feature = "storage-fjall")]
fn batch_parallelism(dispatches: &[&DispatchRecord], sessions: &[&SessionRecord]) -> HealthMetric {
    const NAME: &str = "batch_parallelism";
    const DESC: &str = "Average sessions per dispatch (proxy for concurrent group size)";

    let total_dispatches = dispatches.len();
    if total_dispatches == 0 {
        return unavailable(NAME, DESC, 3.0, 1.5, true);
    }

    // Count sessions per dispatch.
    let mut counts: HashMap<&str, u64> = HashMap::new();
    for s in sessions {
        *counts.entry(s.dispatch_id.as_str()).or_insert(0) += 1;
    }

    // Only include dispatches that have at least one session.
    let dispatches_with_sessions: Vec<u64> = dispatches
        .iter()
        .filter_map(|d| counts.get(d.id.as_str()).copied())
        .collect();

    let n = dispatches_with_sessions.len();
    if n == 0 {
        return unavailable(NAME, DESC, 3.0, 1.5, true);
    }

    let total_sessions: u64 = dispatches_with_sessions.iter().sum();

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "total_sessions and n are bounded; precision loss unreachable at practical scale"
    )]
    let avg = total_sessions as f64 / n as f64;

    #[expect(
        clippy::as_conversions,
        reason = "n fits u64 within SCAN_LIMIT_DISPATCHES"
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
        higher_is_better: true,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- health status display (always compiled) ---

    #[test]
    fn health_status_display() {
        assert_eq!(HealthStatus::Ok.to_string(), "ok");
        assert_eq!(HealthStatus::Warn.to_string(), "warn");
        assert_eq!(HealthStatus::Crit.to_string(), "crit");
        assert_eq!(HealthStatus::Unavailable.to_string(), "unavailable");
    }

    // --- storage-dependent tests ---

    #[cfg(feature = "storage-fjall")]
    #[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
    mod storage_tests {
        use super::*;
        use crate::store::records::{
            DispatchId, DispatchRecord, DispatchStatus, SessionId, SessionRecord,
        };
        use crate::types::SessionStatus;

        fn make_dispatch(id: &str) -> DispatchRecord {
            DispatchRecord {
                id: DispatchId::new(id),
                project: "acme".to_owned(),
                spec: r#"{"prompt_numbers":[1],"project":"acme"}"#.to_owned(),
                status: DispatchStatus::Completed,
                created_at: jiff::Timestamp::now(),
                finished_at: Some(jiff::Timestamp::now()),
                total_cost_usd: 1.0,
                total_sessions: 1,
            }
        }

        fn make_session(dispatch_id: &str, status: SessionStatus) -> SessionRecord {
            SessionRecord {
                id: SessionId::new(aletheia_koina::ulid::Ulid::new().to_string()),
                dispatch_id: DispatchId::new(dispatch_id),
                prompt_number: 1,
                status,
                session_id: None,
                cost_usd: 0.5,
                num_turns: 10,
                duration_ms: 60_000,
                pr_url: None,
                error: None,
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            }
        }

        // --- classify helpers ---

        #[test]
        fn classify_lower_ok() {
            assert_eq!(classify_lower_is_better(0.05, 0.10, 0.20), HealthStatus::Ok);
        }

        #[test]
        fn classify_lower_warn() {
            assert_eq!(
                classify_lower_is_better(0.15, 0.10, 0.20),
                HealthStatus::Warn
            );
        }

        #[test]
        fn classify_lower_crit() {
            assert_eq!(
                classify_lower_is_better(0.25, 0.10, 0.20),
                HealthStatus::Crit
            );
        }

        #[test]
        fn classify_lower_ok_at_boundary() {
            assert_eq!(classify_lower_is_better(0.10, 0.10, 0.20), HealthStatus::Ok);
        }

        #[test]
        fn classify_higher_ok() {
            assert_eq!(
                classify_higher_is_better(0.90, 0.80, 0.60),
                HealthStatus::Ok
            );
        }

        #[test]
        fn classify_higher_warn() {
            assert_eq!(
                classify_higher_is_better(0.70, 0.80, 0.60),
                HealthStatus::Warn
            );
        }

        #[test]
        fn classify_higher_crit() {
            assert_eq!(
                classify_higher_is_better(0.50, 0.80, 0.60),
                HealthStatus::Crit
            );
        }

        #[test]
        fn classify_higher_ok_at_boundary() {
            assert_eq!(
                classify_higher_is_better(0.80, 0.80, 0.60),
                HealthStatus::Ok
            );
        }

        // --- corrective rate ---

        #[test]
        fn corrective_rate_all_clean() {
            let d = make_dispatch("D1");
            let s = make_session("D1", SessionStatus::Success);
            let metric = corrective_rate(&[&d], &[&s]);
            assert_eq!(metric.status, HealthStatus::Ok);
            assert_eq!(metric.value, 0.0);
            assert_eq!(metric.sample_size, 1);
        }

        #[test]
        fn corrective_rate_half_affected() {
            let d1 = make_dispatch("D1");
            let d2 = make_dispatch("D2");
            let s1 = make_session("D1", SessionStatus::Success);
            let s2 = make_session("D2", SessionStatus::Stuck);
            let metric = corrective_rate(&[&d1, &d2], &[&s1, &s2]);
            // 1 out of 2 dispatches has a Stuck session → rate = 0.5 → CRIT
            assert_eq!(metric.status, HealthStatus::Crit);
            assert!((metric.value - 0.5).abs() < 1e-10);
        }

        #[test]
        fn corrective_rate_no_dispatches_unavailable() {
            let metric = corrective_rate(&[], &[]);
            assert_eq!(metric.status, HealthStatus::Unavailable);
            assert_eq!(metric.sample_size, 0);
        }

        // --- stuck rate ---

        #[test]
        fn stuck_rate_zero() {
            let s = make_session("D1", SessionStatus::Success);
            let metric = stuck_rate(&[&s]);
            assert_eq!(metric.status, HealthStatus::Ok);
            assert_eq!(metric.value, 0.0);
        }

        #[test]
        fn stuck_rate_all_stuck_crit() {
            let s1 = make_session("D1", SessionStatus::Stuck);
            let s2 = make_session("D1", SessionStatus::Stuck);
            let metric = stuck_rate(&[&s1, &s2]);
            assert_eq!(metric.status, HealthStatus::Crit);
            assert_eq!(metric.value, 1.0);
            assert_eq!(metric.sample_size, 2);
        }

        #[test]
        fn stuck_rate_below_warn_threshold() {
            // 4 sessions, 0 stuck → 0% < 5% → OK
            let sessions: Vec<SessionRecord> = (0..4)
                .map(|i| {
                    let mut s = make_session("D1", SessionStatus::Success);
                    s.prompt_number = i;
                    s
                })
                .collect();
            let refs: Vec<&SessionRecord> = sessions.iter().collect();
            let metric = stuck_rate(&refs);
            assert_eq!(metric.status, HealthStatus::Ok);
        }

        #[test]
        fn stuck_rate_no_sessions_unavailable() {
            let metric = stuck_rate(&[]);
            assert_eq!(metric.status, HealthStatus::Unavailable);
        }

        // --- cycle time ---

        #[test]
        fn cycle_time_under_4h_ok() {
            let span = jiff::SignedDuration::from_hours(2);
            let now = jiff::Timestamp::now();
            #[expect(clippy::expect_used, reason = "test setup")]
            let start = now.checked_sub(span).expect("test timestamp");
            let d = DispatchRecord {
                id: DispatchId::new("D1"),
                project: "acme".to_owned(),
                spec: "{}".to_owned(),
                status: DispatchStatus::Completed,
                created_at: start,
                finished_at: Some(now),
                total_cost_usd: 0.0,
                total_sessions: 1,
            };
            let metric = cycle_time(&[&d]);
            assert_eq!(metric.status, HealthStatus::Ok);
            assert!(metric.value > 1.9 && metric.value < 2.1);
        }

        #[test]
        fn cycle_time_over_8h_crit() {
            let span = jiff::SignedDuration::from_hours(10);
            let now = jiff::Timestamp::now();
            #[expect(clippy::expect_used, reason = "test setup")]
            let start = now.checked_sub(span).expect("test timestamp");
            let d = DispatchRecord {
                id: DispatchId::new("D1"),
                project: "acme".to_owned(),
                spec: "{}".to_owned(),
                status: DispatchStatus::Completed,
                created_at: start,
                finished_at: Some(now),
                total_cost_usd: 0.0,
                total_sessions: 1,
            };
            let metric = cycle_time(&[&d]);
            assert_eq!(metric.status, HealthStatus::Crit);
            assert!(metric.value > 9.9 && metric.value < 10.1);
        }

        #[test]
        fn cycle_time_no_completed_unavailable() {
            let d = DispatchRecord {
                id: DispatchId::new("D1"),
                project: "acme".to_owned(),
                spec: "{}".to_owned(),
                status: DispatchStatus::Running,
                created_at: jiff::Timestamp::now(),
                finished_at: None,
                total_cost_usd: 0.0,
                total_sessions: 0,
            };
            let metric = cycle_time(&[&d]);
            assert_eq!(metric.status, HealthStatus::Unavailable);
        }

        // --- batch parallelism ---

        #[test]
        fn batch_parallelism_four_sessions_ok() {
            let d = make_dispatch("D1");
            let sessions: Vec<SessionRecord> = (1..=4)
                .map(|i| {
                    let mut s = make_session("D1", SessionStatus::Success);
                    s.prompt_number = i;
                    s
                })
                .collect();
            let refs: Vec<&SessionRecord> = sessions.iter().collect();
            let metric = batch_parallelism(&[&d], &refs);
            assert_eq!(metric.status, HealthStatus::Ok);
            assert_eq!(metric.value, 4.0);
        }

        #[test]
        fn batch_parallelism_one_session_crit() {
            let d = make_dispatch("D1");
            let s = make_session("D1", SessionStatus::Success);
            let metric = batch_parallelism(&[&d], &[&s]);
            assert_eq!(metric.status, HealthStatus::Crit);
            assert_eq!(metric.value, 1.0);
        }

        #[test]
        fn batch_parallelism_no_dispatches_unavailable() {
            let metric = batch_parallelism(&[], &[]);
            assert_eq!(metric.status, HealthStatus::Unavailable);
        }

        // --- observation to issue rate ---

        #[test]
        fn observation_to_issue_rate_always_unavailable() {
            let metric = observation_to_issue_rate();
            assert_eq!(metric.status, HealthStatus::Unavailable);
            assert_eq!(metric.name, "observation_to_issue_rate");
        }
    }
}
