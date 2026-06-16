use super::*;

fn sig(name: &str, args: &str, result: &str) -> ToolCallSignature {
    ToolCallSignature::from_parts(name, args, result)
}

// ── DoomLoopDetector (existing coverage) ──────────────────────────────────

#[test]
fn polling_does_not_trigger() {
    let mut detector = DoomLoopDetector::new(10, 3).unwrap();
    let base = sig("tail", "{\"file\":\"/var/log/syslog\"}", "");

    for i in 0..5 {
        let mut s = base;
        s.result_hash = hash_of(format!("line {i}"));
        detector.record(s).unwrap();
    }
}

#[test]
fn identical_calls_trigger() {
    let mut detector = DoomLoopDetector::new(10, 3).unwrap();
    let s = sig("cat", "{\"file\":\"/etc/hosts\"}", "127.0.0.1 localhost");

    detector.record(s).unwrap();
    detector.record(s).unwrap();
    let err = detector.record(s).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::DoomLoopDetected { k: 3, .. }),
        "expected DoomLoopDetected with k=3, got {err:?}"
    );
}

#[test]
fn ring_capacity_evicts_oldest() {
    let mut detector = DoomLoopDetector::new(10, 3).unwrap();
    let s = sig("echo", "{\"msg\":\"hi\"}", "hi");

    detector.record(s).unwrap();
    detector.record(s).unwrap();
    for _ in 2..11 {
        detector.record(s).unwrap_err();
    }

    assert_eq!(detector.ring.len(), 10);
}

#[test]
fn reset_clears_history() {
    let mut detector = DoomLoopDetector::new(10, 3).unwrap();
    let s = sig("echo", "{\"msg\":\"hi\"}", "hi");

    detector.record(s).unwrap();
    detector.record(s).unwrap();
    detector.record(s).unwrap_err();

    detector.reset();
    detector.record(s).unwrap();
}

#[test]
fn capacity_eviction_avoids_false_trigger() {
    let mut detector = DoomLoopDetector::new(10, 3).unwrap();
    let dup = sig("dup", "{}", "a");

    detector.record(dup).unwrap();
    for i in 0..9 {
        detector
            .record(sig("other", "{}", &format!("{i}")))
            .unwrap();
    }
    detector.record(dup).unwrap();
    detector.record(dup).unwrap();
}

// ── PingPongDetector ──────────────────────────────────────────────────────

#[test]
fn ping_pong_ababa_triggers() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("read", "{\"f\":\"a\"}", "out-a");
    let b = sig("write", "{\"f\":\"b\"}", "out-b");

    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
    let err = det.record(a).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::PingPongDetected { k: 5, .. }),
        "expected PingPongDetected with k=5, got {err:?}"
    );
}

#[test]
fn ping_pong_abcab_does_not_trigger() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("a", "{}", "1");
    let b = sig("b", "{}", "2");
    let c = sig("c", "{}", "3");

    for _ in 0..3 {
        det.record(a).unwrap();
        det.record(b).unwrap();
        det.record(c).unwrap();
    }
}

#[test]
fn ping_pong_same_tool_no_trigger() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("same", "{}", "x");

    for _ in 0..6 {
        det.record(a).unwrap();
    }
}

#[test]
fn ping_pong_abab_does_not_trigger_with_k5() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("a", "{}", "1");
    let b = sig("b", "{}", "2");

    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
    // Only 4 entries — not enough for k=5.
}

#[test]
fn ping_pong_triggers_on_trailing_pattern_after_interruption() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("read", "{}", "r");
    let b = sig("write", "{}", "w");
    let c = sig("search", "{}", "s");

    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(c).unwrap(); // interruption
    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
    let err = det.record(a).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::PingPongDetected { .. }),
        "expected PingPongDetected on trailing A-B-A-B-A, got {err:?}"
    );
}

#[test]
fn ping_pong_capacity_evicts_oldest() {
    let mut det = PingPongDetector::new(5, 5).unwrap();
    let a = sig("a", "{}", "1");
    let b = sig("b", "{}", "2");

    // Fill to capacity with A-B-A-B (4 entries).
    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
    // Evict the first A.
    det.record(sig("c", "{}", "3")).unwrap();
    // Ring is now B-A-B-C (4 entries). Adding A gives A-B-C-A, not a ping-pong.
    det.record(a).unwrap();
}

#[test]
fn ping_pong_reset_clears() {
    let mut det = PingPongDetector::new(10, 5).unwrap();
    let a = sig("a", "{}", "1");
    let b = sig("b", "{}", "2");

    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap_err();

    det.reset();
    // After reset, only 4 entries — not enough for k=5.
    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    det.record(b).unwrap();
}

#[test]
fn ping_pong_capacity_one_never_detects() {
    let mut det = PingPongDetector::new(1, 3).unwrap();
    let a = sig("a", "{}", "1");
    let b = sig("b", "{}", "2");

    det.record(a).unwrap();
    det.record(b).unwrap();
    det.record(a).unwrap();
    // Ring only ever holds 1 element — never enough for k=3.
}

// ── NoProgressDetector ────────────────────────────────────────────────────

#[test]
fn no_progress_same_hash_three_turns_triggers() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(42, true).unwrap();
    det.record_turn(42, true).unwrap();
    let err = det.record_turn(42, true).unwrap_err();

    assert!(
        matches!(
            err,
            LoopDetectorError::NoProgressDetected {
                consecutive: 3,
                limit: 3
            }
        ),
        "expected NoProgressDetected at limit 3, got {err:?}"
    );
}

#[test]
fn no_progress_different_hash_resets() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(42, true).unwrap();
    det.record_turn(42, true).unwrap();
    det.record_turn(99, true).unwrap(); // different hash resets
    det.record_turn(99, true).unwrap();
    det.record_turn(99, true).unwrap_err();
}

#[test]
fn no_progress_no_tool_does_not_increment_nor_reset() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(42, true).unwrap();
    det.record_turn(42, true).unwrap();
    // No-tool turn: counter stays at 2, last_hash stays at 42.
    det.record_turn(999, false).unwrap();
    // Next tool turn with same hash should trigger.
    let err = det.record_turn(42, true).unwrap_err();
    assert!(
        matches!(
            err,
            LoopDetectorError::NoProgressDetected { consecutive: 3, .. }
        ),
        "expected NoProgressDetected after no-tool gap, got {err:?}"
    );
}

#[test]
fn no_progress_no_tool_streak_never_triggers() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(1, false).unwrap();
    det.record_turn(1, false).unwrap();
    det.record_turn(1, false).unwrap();
    det.record_turn(1, false).unwrap();
    // Counter stays at 0 because no tool was ever called.
}

#[test]
fn no_progress_reset_clears() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(42, true).unwrap();
    det.record_turn(42, true).unwrap();
    det.reset();
    det.record_turn(42, true).unwrap(); // should not fire after reset
}

#[test]
fn no_progress_first_tool_turn_sets_counter_to_one() {
    let mut det = NoProgressDetector::new(3).unwrap();
    det.record_turn(42, true).unwrap();
    assert_eq!(det.consecutive_no_progress, 1);
}

// ── LoopGuard ─────────────────────────────────────────────────────────────

#[test]
fn loop_guard_doom_wins_over_ping_pong() {
    // Sequence: A, A, A — doom fires at the 3rd A (3 identical).
    // Ping-pong requires A-B-A-B-A, so it never fires here.
    let mut guard = LoopGuard::with_limits(3, 5, 3).unwrap();
    let tc = [("tool", "{}", "res")];

    guard.record("ok", "", &tc).unwrap();
    guard.record("ok", "", &tc).unwrap();
    let err = guard.record("ok", "", &tc).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::DoomLoopDetected { .. }),
        "expected DoomLoopDetected, got {err:?}"
    );
}

#[test]
fn loop_guard_ping_pong_on_alternation() {
    let mut guard = LoopGuard::with_limits(10, 5, 3).unwrap();
    let a = [("read", "{}", "r")];
    let b = [("write", "{}", "w")];

    // Vary content so no-progress does not fire first.
    guard.record("turn 1", "", &a).unwrap();
    guard.record("turn 2", "", &b).unwrap();
    guard.record("turn 3", "", &a).unwrap();
    guard.record("turn 4", "", &b).unwrap();
    let err = guard.record("turn 5", "", &a).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::PingPongDetected { .. }),
        "expected PingPongDetected, got {err:?}"
    );
}

#[test]
fn loop_guard_no_progress_on_repeated_content() {
    let mut guard = LoopGuard::with_limits(10, 5, 3).unwrap();
    let tc = [("tool", "{}", "res")];

    guard.record("same", "", &tc).unwrap();
    guard.record("same", "", &tc).unwrap();
    let err = guard.record("same", "", &tc).unwrap_err();

    assert!(
        matches!(err, LoopDetectorError::NoProgressDetected { .. }),
        "expected NoProgressDetected, got {err:?}"
    );
}

#[test]
fn loop_guard_no_progress_no_tool_never_fires() {
    let mut guard = LoopGuard::with_limits(10, 5, 3).unwrap();
    let tc: &[(&str, &str, &str)] = &[];

    guard.record("same", "", tc).unwrap();
    guard.record("same", "", tc).unwrap();
    guard.record("same", "", tc).unwrap();
    guard.record("same", "", tc).unwrap();
    // No tool calls → no-progress counter stays at 0.
}

#[test]
fn loop_guard_multiple_tools_per_turn() {
    let mut guard = LoopGuard::with_limits(10, 5, 3).unwrap();
    let tc1 = [("a", "{}", "1"), ("b", "{}", "2")];
    let tc2 = [("c", "{}", "3")];
    let tc3 = [("d", "{}", "4")];

    // Vary content and tools so no detector fires.
    guard.record("ok1", "", &tc1).unwrap();
    guard.record("ok2", "", &tc2).unwrap();
    guard.record("ok3", "", &tc3).unwrap();
}

#[test]
fn loop_guard_reset_on_user_message_clears_all() {
    let mut guard = LoopGuard::with_limits(3, 5, 3).unwrap();
    let tc = [("tool", "{}", "res")];

    guard.record("ok", "", &tc).unwrap();
    guard.record("ok", "", &tc).unwrap();
    guard.reset_on_user_message();
    guard.record("ok", "", &tc).unwrap();
    guard.record("ok", "", &tc).unwrap();
    // Would have fired on the 3rd call before reset.
    guard.record("ok", "", &tc).unwrap_err();
}

#[test]
fn loop_guard_reasoning_changes_hash() {
    let mut guard = LoopGuard::with_limits(10, 5, 2).unwrap();
    let tc = [("tool", "{}", "res")];

    guard.record("ok", "think A", &tc).unwrap();
    guard.record("ok", "think B", &tc).unwrap();
    // Different reasoning → different hash → no trigger.
}

#[test]
fn loop_guard_empty_tool_calls_ok() {
    let mut guard = LoopGuard::new();
    let tc: &[(&str, &str, &str)] = &[];

    guard.record("hello", "", tc).unwrap();
    guard.record("world", "", tc).unwrap();
}

// ── Adversarial / property-style ──────────────────────────────────────────

#[test]
fn legitimate_workflow_no_false_positives() {
    let mut guard = LoopGuard::with_limits(3, 5, 3).unwrap();
    let ops: Vec<(&str, &str, &str)> = vec![
        ("read", "{\"f\":\"a\"}", "a"),
        ("read", "{\"f\":\"b\"}", "b"),
        ("write", "{\"f\":\"c\"}", "c"),
        ("read", "{\"f\":\"d\"}", "d"),
        ("write", "{\"f\":\"e\"}", "e"),
    ];

    for (i, (name, args, res)) in ops.iter().cycle().take(100).enumerate() {
        let tc = [(*name, *args, *res)];
        guard.record(&format!("turn {i}"), "", &tc).unwrap();
    }
}

#[test]
fn ping_pong_trailing_after_interruption() {
    // [A,B,A,B,C,A,B,A,B,A] → ping-pong fires on trailing A-B-A-B-A.
    let mut det = PingPongDetector::new(12, 5).unwrap();
    let a = sig("read", "{}", "r");
    let b = sig("write", "{}", "w");
    let c = sig("search", "{}", "s");

    let seq = [a, b, a, b, c, a, b, a, b, a];
    let mut last = None;
    for s in seq {
        last = det.record(s).err();
    }
    assert!(
        matches!(last, Some(LoopDetectorError::PingPongDetected { .. })),
        "expected PingPongDetected on trailing pattern"
    );
}

#[test]
fn doom_and_ping_pong_ordering_documented() {
    // With the same signature sequence, doom is checked before ping-pong
    // per tool-call record. This test documents that ordering.
    let mut guard = LoopGuard::with_limits(3, 3, 3).unwrap();
    let a = [("tool", "{}", "same")];

    guard.record("ok", "", &a).unwrap();
    guard.record("ok", "", &a).unwrap();
    let err = guard.record("ok", "", &a).unwrap_err();

    // Doom fires first because it's 3 consecutive identical signatures.
    // Ping-pong with k=3 would need A-B-A, which this is not.
    assert!(
        matches!(err, LoopDetectorError::DoomLoopDetected { k: 3, .. }),
        "doom must fire before ping-pong on identical sequences"
    );
}

// ── Invalid configuration ─────────────────────────────────────────────────

#[test]
fn doom_loop_detector_rejects_zero_capacity() {
    let err = DoomLoopDetector::new(0, 3).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for zero capacity, got {err:?}"
    );
}

#[test]
fn doom_loop_detector_rejects_zero_k() {
    let err = DoomLoopDetector::new(10, 0).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for zero k, got {err:?}"
    );
}

#[test]
fn ping_pong_detector_rejects_zero_capacity() {
    let err = PingPongDetector::new(0, 3).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for zero capacity, got {err:?}"
    );
}

#[test]
fn ping_pong_detector_rejects_k_below_three() {
    let err = PingPongDetector::new(10, 2).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for k < 3, got {err:?}"
    );
}

#[test]
fn no_progress_detector_rejects_zero_limit() {
    let err = NoProgressDetector::new(0).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for zero limit, got {err:?}"
    );
}

#[test]
fn loop_guard_with_limits_rejects_invalid_doom_k() {
    let err = LoopGuard::with_limits(0, 5, 3).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for doom_k == 0, got {err:?}"
    );
}

#[test]
fn loop_guard_with_limits_rejects_invalid_ping_pong_k() {
    let err = LoopGuard::with_limits(3, 2, 3).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for ping_pong_k < 3, got {err:?}"
    );
}

#[test]
fn loop_guard_with_limits_rejects_invalid_no_progress_limit() {
    let err = LoopGuard::with_limits(3, 5, 0).unwrap_err();
    assert!(
        matches!(err, LoopDetectorError::InvalidConfig { .. }),
        "expected InvalidConfig for no_progress_limit == 0, got {err:?}"
    );
}
