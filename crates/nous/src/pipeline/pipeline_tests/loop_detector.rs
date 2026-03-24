use super::*;

fn is_ok(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Ok)
}

fn is_warn(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Warn { .. })
}

fn is_halt(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Halt { .. })
}

// --- Same-args repetition ---

#[test]
fn loop_detector_no_loop() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "hash1", false)),
        "unique calls should not trigger loop"
    );
    assert!(
        is_ok(&det.record("read", "hash2", false)),
        "different tool should not trigger loop"
    );
    assert!(
        is_ok(&det.record("exec", "hash3", false)),
        "different hash should not trigger loop"
    );
}

#[test]
fn loop_detector_detects_repeat() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    let result = det.record("exec", "same", false);
    assert!(is_warn(&result), "third repeat should trigger loop warning");
    match result {
        LoopVerdict::Warn { pattern, .. } => {
            assert_eq!(pattern, "exec:same", "pattern should be tool:hash");
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn loop_detector_different_inputs_ok() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "hash1", false)),
        "unique hash1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "hash2", false)),
        "unique hash2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "hash3", false)),
        "unique hash3 should not trigger"
    );
}

#[test]
fn loop_detector_reset() {
    let mut det = LoopDetector::new(3);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    det.reset();
    assert_eq!(det.call_count(), 0, "call count should be zero after reset");
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "reset should clear history"
    );
}

#[test]
fn loop_detector_threshold_4() {
    let mut det = LoopDetector::new(4);
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 1 of 4 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 2 of 4 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 3 of 4 should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "repeat 4 of 4 should trigger loop warning"
    );
}

#[test]
fn loop_detector_threshold_1() {
    let mut det = LoopDetector::new(1);
    let result = det.record("exec", "hash", false);
    assert!(
        is_warn(&result),
        "threshold 1 should trigger warning on first call"
    );
}

// --- Warning/halt escalation ---

#[test]
fn loop_detector_warn_then_halt() {
    let mut det = LoopDetector::with_limits(3, 4, 2);

    // WHY: after threshold (3) identical calls, every subsequent identical call
    // re-triggers detection because the tail always has >= 3 consecutive matches.
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "first detection should warn");
    assert_eq!(det.warnings_issued(), 1);

    // Second identical call triggers again immediately
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "second detection should still warn");
    assert_eq!(det.warnings_issued(), 2);

    // Third detection should halt (max_warnings=2 exceeded)
    let v = det.record("exec", "same", false);
    assert!(
        is_halt(&v),
        "third detection should halt after max warnings"
    );
}

#[test]
fn loop_detector_reset_clears_warnings() {
    let mut det = LoopDetector::with_limits(2, 4, 1);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "should warn");
    assert_eq!(det.warnings_issued(), 1);

    det.reset();
    assert_eq!(det.warnings_issued(), 0, "reset should clear warning count");
}

// --- Alternating failure detection ---

#[test]
fn loop_detector_detects_alternating_failure() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    det.record("tool_a", "h1", true);
    det.record("tool_b", "h2", true);
    det.record("tool_a", "h3", true);
    det.record("tool_b", "h4", true);
    det.record("tool_a", "h5", true);
    let v = det.record("tool_b", "h6", true);
    assert!(
        is_warn(&v),
        "alternating failure pattern should be detected"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("alternating"),
                "pattern should mention alternating: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn alternating_failure_not_triggered_without_errors() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    det.record("tool_a", "h1", false);
    det.record("tool_b", "h2", false);
    det.record("tool_a", "h3", false);
    det.record("tool_b", "h4", false);
    det.record("tool_a", "h5", false);
    let v = det.record("tool_b", "h6", false);
    // NOTE: this might trigger cycle detection instead, but NOT alternating failure
    // since is_error is false. The cycle detection may or may not fire depending on
    // whether the signatures form a recognized cycle.
    if let LoopVerdict::Warn { pattern, .. } = &v {
        assert!(
            !pattern.contains("alternating"),
            "non-error pattern should not be alternating failures"
        );
    }
}

#[test]
fn alternating_failure_same_tool_not_triggered() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    // Same tool alternating with itself should not trigger alternating failure
    for _ in 0..6 {
        det.record("tool_a", "h1", true);
    }
    // This should trigger same-args or consecutive errors, but NOT alternating failure
}

// --- Consecutive error detection ---

#[test]
fn loop_detector_detects_consecutive_errors_same_tool() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("exec", "h2", true);
    det.record("exec", "h3", true);
    let v = det.record("exec", "h4", true);
    assert!(
        is_warn(&v),
        "4 consecutive errors from same tool should trigger"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("escalating retries"),
                "pattern should mention escalating retries: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn consecutive_errors_different_tools() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("read", "h2", true);
    det.record("write", "h3", true);
    let v = det.record("search", "h4", true);
    assert!(
        is_warn(&v),
        "4 consecutive errors from mixed tools should trigger"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("consecutive failures"),
                "pattern should mention consecutive failures: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn consecutive_errors_broken_by_success() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("exec", "h2", true);
    det.record("exec", "h3", false); // success breaks the streak
    let v = det.record("exec", "h4", true);
    assert!(is_ok(&v), "success should reset consecutive error count");
}

// --- Cycle detection ---

#[test]
fn loop_detector_detects_ab_cycle() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "b", false)),
        "cycle step b-1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "b", false)),
        "cycle step b-2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-3 should not trigger"
    );
    let result = det.record("exec", "b", false);
    assert!(
        is_warn(&result),
        "2-step cycle repeated 3 times should be detected"
    );
}

#[test]
fn loop_detector_detects_abc_cycle() {
    let mut det = LoopDetector::new(3);
    // A -> B -> C repeated 3 times
    for _ in 0..2 {
        det.record("a", "1", false);
        det.record("b", "2", false);
        det.record("c", "3", false);
    }
    det.record("a", "1", false);
    det.record("b", "2", false);
    let v = det.record("c", "3", false);
    assert!(
        is_warn(&v),
        "3-step cycle repeated 3 times should be detected"
    );
}

#[test]
fn loop_detector_non_repeating_interleaved_not_detected() {
    let mut det = LoopDetector::new(3);
    for i in 0..6 {
        assert!(
            is_ok(&det.record("exec", &format!("hash{i}"), false)),
            "unique hash should not trigger loop"
        );
    }
}

// --- Utility method tests ---

#[test]
fn loop_detector_call_count_tracks() {
    let mut det = LoopDetector::new(10);
    det.record("a", "1", false);
    det.record("b", "2", false);
    det.record("c", "3", false);
    assert_eq!(
        det.call_count(),
        3,
        "call count should track all recordings"
    );
}

#[test]
fn loop_detector_window_cap_evicts_old_calls() {
    let mut det = LoopDetector::new(100);
    for i in 0..55 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert_eq!(
        det.call_count(),
        DEFAULT_LOOP_WINDOW,
        "history should be capped at window size"
    );
}

#[test]
fn loop_detector_pattern_count_tracks_repetitions() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    assert_eq!(
        det.pattern_count(),
        3,
        "pattern count should track consecutive repeats"
    );
}

#[test]
fn loop_detector_pattern_count_zero_on_empty() {
    let det = LoopDetector::new(3);
    assert_eq!(
        det.pattern_count(),
        0,
        "empty detector should have zero pattern count"
    );
}

#[test]
fn loop_detector_pattern_count_resets_on_different() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "hash1", false);
    det.record("exec", "hash1", false);
    det.record("read", "hash2", false);
    assert_eq!(det.pattern_count(), 1, "different call breaks the streak");
}

#[test]
fn loop_detector_many_unique_then_repeat() {
    let mut det = LoopDetector::new(3);
    for i in 0..20 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat after unique should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "third repeat should trigger loop"
    );
}

#[test]
fn loop_detector_window_still_detects_loops() {
    let mut det = LoopDetector::new(3);
    for i in 0..18 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "should detect loop even after window eviction"
    );
}

#[test]
fn loop_detector_reset_clears_pattern_count() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    assert_eq!(
        det.pattern_count(),
        2,
        "should have 2 repetitions before reset"
    );
    det.reset();
    assert_eq!(
        det.pattern_count(),
        0,
        "pattern count should be zero after reset"
    );
    assert_eq!(det.call_count(), 0, "call count should be zero after reset");
}

// --- LoopVerdict tests ---

#[test]
fn loop_verdict_equality() {
    assert_eq!(LoopVerdict::Ok, LoopVerdict::Ok);
    assert_ne!(
        LoopVerdict::Ok,
        LoopVerdict::Warn {
            pattern: "x".to_owned(),
            message: "y".to_owned(),
        }
    );
}

#[test]
fn loop_verdict_warn_contains_pattern_in_message() {
    let mut det = LoopDetector::new(2);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    match v {
        LoopVerdict::Warn { pattern, message } => {
            assert!(
                message.contains(&pattern),
                "warning message should contain pattern"
            );
        }
        _ => panic!("expected Warn"),
    }
}
