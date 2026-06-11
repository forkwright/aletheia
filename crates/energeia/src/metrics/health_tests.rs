use super::*;

#[test]
fn health_status_display() {
    assert_eq!(HealthStatus::Ok.to_string(), "ok");
    assert_eq!(HealthStatus::Warn.to_string(), "warn");
    assert_eq!(HealthStatus::Crit.to_string(), "crit");
    assert_eq!(HealthStatus::Unavailable.to_string(), "unavailable");
}

#[cfg(feature = "storage-fjall")]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod storage_tests {
    use super::*;
    use crate::store::records::{
        DispatchId, DispatchRecord, DispatchStatus, QaVerdictRecord, SessionId, SessionRecord,
    };
    use crate::types::{QaVerdict, SessionStatus};

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
            id: SessionId::new(koina::ulid::Ulid::new().to_string()),
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

    fn make_qa_verdict(dispatch_id: &str, verdict: QaVerdict) -> QaVerdictRecord {
        QaVerdictRecord {
            dispatch_id: DispatchId::new(dispatch_id),
            project: "acme".to_owned(),
            verdict,
            recorded_at: jiff::Timestamp::now(),
        }
    }

    // ── classify helpers ──

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

    // ── corrective rate ──

    #[test]
    fn corrective_rate_all_clean() {
        let d = make_dispatch("D1");
        let s = make_session("D1", SessionStatus::Success);
        let metric = corrective_rate(&[&d], &[&s], &[]);
        assert_eq!(metric.status, HealthStatus::Ok);
        assert_eq!(metric.value, 0.0);
        assert_eq!(metric.sample_size, 1);
        assert!(
            metric.is_proxied,
            "without stored QA verdicts corrective_rate uses session failures as a proxy"
        );
    }

    #[test]
    fn corrective_rate_half_affected() {
        let d1 = make_dispatch("D1");
        let d2 = make_dispatch("D2");
        let s1 = make_session("D1", SessionStatus::Success);
        let s2 = make_session("D2", SessionStatus::Stuck);
        let metric = corrective_rate(&[&d1, &d2], &[&s1, &s2], &[]);
        // 1 out of 2 dispatches has a Stuck session → rate = 0.5 → CRIT
        assert_eq!(metric.status, HealthStatus::Crit);
        assert!((metric.value - 0.5).abs() < 1e-10);
        assert!(metric.is_proxied);
    }

    #[test]
    fn corrective_rate_prefers_recorded_qa_verdicts_over_proxy() {
        let d = make_dispatch("D1");
        let s = make_session("D1", SessionStatus::Stuck);
        let pass = make_qa_verdict("D1", QaVerdict::Pass);
        let metric = corrective_rate(&[&d], &[&s], &[pass]);
        assert_eq!(
            metric.status,
            HealthStatus::Ok,
            "stored QA Pass verdict should not fall back to the stuck-session proxy"
        );
        assert_eq!(metric.value, 0.0);
        assert!(
            !metric.is_proxied,
            "stored QA verdicts are direct corrective-rate data"
        );
    }

    #[test]
    fn corrective_rate_no_dispatches_unavailable() {
        let metric = corrective_rate(&[], &[], &[]);
        assert_eq!(metric.status, HealthStatus::Unavailable);
        assert_eq!(metric.sample_size, 0);
        assert!(!metric.is_proxied);
    }

    // ── stuck rate ──

    #[test]
    fn stuck_rate_zero() {
        let s = make_session("D1", SessionStatus::Success);
        let metric = stuck_rate(&[&s]);
        assert_eq!(metric.status, HealthStatus::Ok);
        assert_eq!(metric.value, 0.0);
        assert!(!metric.is_proxied);
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

    // ── cycle time ──

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
        assert!(!metric.is_proxied);
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

    // ── batch parallelism ──

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
        assert!(metric.is_proxied);
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

    #[test]
    fn observation_to_issue_rate_always_unavailable() {
        let metric = observation_to_issue_rate();
        assert_eq!(metric.status, HealthStatus::Unavailable);
        assert_eq!(metric.name, "observation_to_issue_rate");
    }
}
