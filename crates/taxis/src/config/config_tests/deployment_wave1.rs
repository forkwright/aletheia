//! Pins deployment-tunable config defaults and serde override behavior.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

#[test]
fn timeouts_default_matches_koina_const() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.timeouts.llm_call_secs,
        koina::defaults::TIMEOUT_SECONDS,
        "default llm_call_secs must equal koina::defaults::TIMEOUT_SECONDS"
    );
    assert_eq!(
        config.timeouts.llm_call_secs, 300,
        "default llm_call_secs must be 300 seconds"
    );
    assert_eq!(
        config.timeouts.approval_secs, DEFAULT_APPROVAL_TIMEOUT_SECS,
        "default approval_secs must equal the owned approval timeout default"
    );
    assert_eq!(
        config.timeouts.approval_secs, 120,
        "default approval_secs must be 120 seconds"
    );
}

#[test]
fn capacity_defaults_match_koina_consts() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.capacity.max_tool_output_bytes,
        koina::defaults::MAX_OUTPUT_BYTES,
        "default max_tool_output_bytes must equal koina::defaults::MAX_OUTPUT_BYTES"
    );
    assert_eq!(
        config.capacity.max_tool_output_bytes, 51_200,
        "default max_tool_output_bytes must be 51200 (50 KiB)"
    );
}

#[test]
fn retry_defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.retry.max_attempts, 3,
        "default max_attempts must be 3"
    );
    assert_eq!(
        config.retry.backoff_base_ms, 1_000,
        "default backoff_base_ms must be 1000"
    );
    assert_eq!(
        config.retry.backoff_max_ms, 30_000,
        "default backoff_max_ms must be 30 000"
    );
    assert!(
        config.retry.backoff_max_ms >= config.retry.backoff_base_ms,
        "backoff_max_ms must be >= backoff_base_ms"
    );
}

#[test]
fn timeouts_override_from_json() {
    let json = r#"{"timeouts": {"llmCallSecs": 600, "approvalSecs": 45}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse timeouts override");
    assert_eq!(
        config.timeouts.llm_call_secs, 600,
        "llm_call_secs override from json should take effect"
    );
    assert_eq!(
        config.timeouts.approval_secs, 45,
        "approval_secs override from json should take effect"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "unrelated gateway port should remain at default"
    );
}

#[test]
fn capacity_override_from_json() {
    let json = r#"{"capacity": {"maxToolOutputBytes": 102400}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse capacity override");
    assert_eq!(
        config.capacity.max_tool_output_bytes, 102_400,
        "max_tool_output_bytes override from json should take effect"
    );
}

#[test]
fn retry_override_from_json() {
    let json = r#"{"retry": {"maxAttempts": 5, "backoffBaseMs": 2000, "backoffMaxMs": 60000}}"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse retry override");
    assert_eq!(
        config.retry.max_attempts, 5,
        "max_attempts override from json should take effect"
    );
    assert_eq!(
        config.retry.backoff_base_ms, 2_000,
        "backoff_base_ms override from json should take effect"
    );
    assert_eq!(
        config.retry.backoff_max_ms, 60_000,
        "backoff_max_ms override from json should take effect"
    );
}

#[test]
fn new_sections_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.timeouts.llm_call_secs = 120;
    config.timeouts.approval_secs = 45;
    config.capacity.max_tool_output_bytes = 8192;
    config.retry.max_attempts = 1;
    config.retry.backoff_base_ms = 500;
    config.retry.backoff_max_ms = 5_000;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.timeouts.llm_call_secs, 120,
        "llm_call_secs should survive serde roundtrip"
    );
    assert_eq!(
        back.timeouts.approval_secs, 45,
        "approval_secs should survive serde roundtrip"
    );
    assert_eq!(
        back.capacity.max_tool_output_bytes, 8192,
        "max_tool_output_bytes should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.max_attempts, 1,
        "max_attempts should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.backoff_base_ms, 500,
        "backoff_base_ms should survive serde roundtrip"
    );
    assert_eq!(
        back.retry.backoff_max_ms, 5_000,
        "backoff_max_ms should survive serde roundtrip"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one assertion per pre-extraction constant; splitting would fragment the regression guard"
)]
fn deployment_defaults_match_original_constants() {
    let nb = NousBehaviorConfig::default();
    assert_eq!(nb.degraded_panic_threshold, 5, "degraded_panic_threshold");
    assert_eq!(nb.degraded_window_secs, 600, "degraded_window_secs");
    assert_eq!(nb.inbox_recv_timeout_secs, 30, "inbox_recv_timeout_secs");
    assert_eq!(
        nb.consecutive_timeout_warn_threshold, 3,
        "consecutive_timeout_warn_threshold"
    );
    assert_eq!(nb.max_spawned_tasks, 8, "max_spawned_tasks");
    assert_eq!(nb.gc_interval_secs, 300, "gc_interval_secs");
    assert_eq!(nb.manager_dead_threshold, 3, "manager_dead_threshold");
    assert_eq!(
        nb.manager_max_restart_backoff_secs, 300,
        "manager_max_restart_backoff_secs"
    );
    assert_eq!(
        nb.manager_restart_drain_timeout_secs, 30,
        "manager_restart_drain_timeout_secs"
    );
    assert_eq!(
        nb.manager_restart_decay_window_secs, 3_600,
        "manager_restart_decay_window_secs"
    );
    assert_eq!(
        nb.manager_health_interval_secs, 30,
        "manager_health_interval_secs"
    );
    assert_eq!(nb.manager_ping_timeout_secs, 5, "manager_ping_timeout_secs");
    assert_eq!(nb.stuck_turn_timeout_secs, 600, "stuck_turn_timeout_secs");
    assert_eq!(nb.loop_detection_window, 50, "loop_detection_window");
    assert_eq!(nb.cycle_detection_max_len, 10, "cycle_detection_max_len");
    assert_eq!(
        nb.self_audit_event_threshold, 50,
        "self_audit_event_threshold"
    );

    let kc = KnowledgeConfig::default();
    assert_eq!(
        kc.conflict_max_llm_calls_per_fact, 3,
        "conflict_max_llm_calls_per_fact"
    );
    assert!(
        (kc.conflict_intra_batch_dedup_threshold - 0.95).abs() < f64::EPSILON,
        "conflict_intra_batch_dedup_threshold"
    );
    assert!(
        (kc.conflict_candidate_distance_threshold - 0.28).abs() < f64::EPSILON,
        "conflict_candidate_distance_threshold"
    );
    assert_eq!(kc.conflict_max_candidates, 5, "conflict_max_candidates");
    assert!(
        (kc.decay_reinforcement_boost - 0.02).abs() < f64::EPSILON,
        "decay_reinforcement_boost"
    );
    assert!(
        (kc.decay_max_reinforcement_bonus - 1.0).abs() < f64::EPSILON,
        "decay_max_reinforcement_bonus"
    );
    assert!(
        (kc.decay_cross_agent_bonus_per_agent - 0.15).abs() < f64::EPSILON,
        "decay_cross_agent_bonus_per_agent"
    );
    assert!(
        (kc.decay_max_cross_agent_multiplier - 1.75).abs() < f64::EPSILON,
        "decay_max_cross_agent_multiplier"
    );
    assert!(
        (kc.extraction_confidence_threshold - 0.3).abs() < f64::EPSILON,
        "extraction_confidence_threshold"
    );
    assert_eq!(
        kc.extraction_min_fact_length, 10,
        "extraction_min_fact_length"
    );
    assert_eq!(
        kc.extraction_max_fact_length, 500,
        "extraction_max_fact_length"
    );
    assert_eq!(kc.instinct_min_tool_calls, 5, "instinct_min_tool_calls");

    let pb = ProviderBehaviorConfig::default();
    assert_eq!(
        pb.non_streaming_timeout_secs, 120,
        "non_streaming_timeout_secs"
    );
    assert_eq!(pb.sse_default_retry_ms, 1_000, "sse_default_retry_ms");
    assert!(
        (pb.concurrency_ewma_alpha - 0.8).abs() < f64::EPSILON,
        "concurrency_ewma_alpha"
    );
    assert!(
        (pb.concurrency_latency_threshold_secs - 30.0).abs() < f64::EPSILON,
        "concurrency_latency_threshold_secs"
    );
    assert!(!pb.complexity_routing_enabled, "complexity_routing_enabled");
    assert_eq!(pb.complexity_low_threshold, 30, "complexity_low_threshold");
    assert_eq!(
        pb.complexity_high_threshold, 70,
        "complexity_high_threshold"
    );

    let al = ApiLimitsConfig::default();
    assert_eq!(al.max_session_name_len, 255, "max_session_name_len");
    assert_eq!(al.max_identifier_bytes, 256, "max_identifier_bytes");
    assert_eq!(al.max_history_limit, 1_000, "max_history_limit");
    assert_eq!(al.default_history_limit, 50, "default_history_limit");
    assert_eq!(al.max_message_bytes, 262_144, "max_message_bytes");
    assert_eq!(al.max_facts_limit, 1_000, "max_facts_limit");
    assert_eq!(al.max_search_limit, 1_000, "max_search_limit");
    assert_eq!(al.max_import_batch_size, 1_000, "max_import_batch_size");
    assert_eq!(al.idempotency_ttl_secs, 300, "idempotency_ttl_secs");
    assert_eq!(al.idempotency_capacity, 10_000, "idempotency_capacity");
    assert_eq!(
        al.idempotency_max_key_length, 64,
        "idempotency_max_key_length"
    );
    assert_eq!(al.clock_skew_leeway_secs, 30, "clock_skew_leeway_secs");
    assert_eq!(
        al.expiry_warning_threshold_secs, 3_600,
        "expiry_warning_threshold_secs"
    );

    let db = DaemonBehaviorConfig::default();
    assert_eq!(
        db.watchdog_backoff_base_secs, 2,
        "watchdog_backoff_base_secs"
    );
    assert_eq!(
        db.watchdog_backoff_cap_secs, 300,
        "watchdog_backoff_cap_secs"
    );
    assert_eq!(
        db.prosoche_anomaly_sample_size, 15,
        "prosoche_anomaly_sample_size"
    );
    assert_eq!(
        db.runner_output_mode,
        DaemonRunnerOutputMode::Summary,
        "runner_output_mode"
    );
    assert_eq!(
        db.runner_output_brief_head_lines, 5,
        "runner_output_brief_head_lines"
    );
    assert_eq!(
        db.runner_output_brief_tail_lines, 3,
        "runner_output_brief_tail_lines"
    );

    let tl = ToolLimitsConfig::default();
    assert_eq!(tl.max_pattern_length, 1_000, "max_pattern_length");
    assert_eq!(tl.subprocess_timeout_secs, 60, "subprocess_timeout_secs");
    assert_eq!(tl.max_write_bytes, 10_485_760, "max_write_bytes");
    assert_eq!(tl.max_read_bytes, 52_428_800, "max_read_bytes");
    assert_eq!(tl.max_command_length, 10_000, "max_command_length");
    assert_eq!(tl.message_max_len, 4_000, "message_max_len");
    assert_eq!(
        tl.inter_session_max_message_len, 100_000,
        "inter_session_max_message_len"
    );
    assert_eq!(
        tl.inter_session_max_timeout_secs, 300,
        "inter_session_max_timeout_secs"
    );

    let mc = MessagingConfig::default();
    assert_eq!(mc.poll_interval_ms, 2_000, "poll_interval_ms");
    assert_eq!(mc.buffer_capacity, 100, "buffer_capacity");
    assert_eq!(mc.circuit_breaker_threshold, 5, "circuit_breaker_threshold");
    assert_eq!(
        mc.halted_health_check_interval_secs, 60,
        "halted_health_check_interval_secs"
    );
    assert_eq!(mc.rpc_timeout_secs, 10, "rpc_timeout_secs");
    assert_eq!(mc.health_timeout_secs, 2, "health_timeout_secs");
    assert_eq!(mc.receive_timeout_secs, 15, "receive_timeout_secs");
    assert_eq!(
        mc.agent_dispatch_timeout_secs, 300,
        "agent_dispatch_timeout_secs"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one assertion per pre-extraction constant; splitting would fragment the regression guard"
)]
fn per_agent_defaults_match_original_constants() {
    let ab = AgentBehaviorDefaults::default();

    // Safety
    assert_eq!(
        ab.safety_loop_detection_threshold, 3,
        "safety_loop_detection_threshold"
    );
    assert_eq!(
        ab.safety_consecutive_error_threshold, 4,
        "safety_consecutive_error_threshold"
    );
    assert_eq!(ab.safety_loop_max_warnings, 2, "safety_loop_max_warnings");
    assert_eq!(
        ab.safety_session_token_cap, 500_000,
        "safety_session_token_cap"
    );
    assert_eq!(
        ab.safety_max_consecutive_tool_only_iterations, 3,
        "safety_max_consecutive_tool_only_iterations"
    );

    // Hooks
    assert!(ab.hooks_cost_control_enabled, "hooks_cost_control_enabled");
    assert_eq!(ab.hooks_turn_token_budget, 0, "hooks_turn_token_budget");
    assert!(
        ab.hooks_scope_enforcement_enabled,
        "hooks_scope_enforcement_enabled"
    );
    assert!(
        ab.hooks_correction_hooks_enabled,
        "hooks_correction_hooks_enabled"
    );
    assert!(
        ab.hooks_audit_logging_enabled,
        "hooks_audit_logging_enabled"
    );

    // Distillation
    assert_eq!(
        ab.distillation_context_token_trigger, 120_000,
        "distillation_context_token_trigger"
    );
    assert_eq!(
        ab.distillation_message_count_trigger, 150,
        "distillation_message_count_trigger"
    );
    assert_eq!(
        ab.distillation_stale_session_days, 7,
        "distillation_stale_session_days"
    );
    assert_eq!(
        ab.distillation_stale_min_messages, 20,
        "distillation_stale_min_messages"
    );
    assert_eq!(
        ab.distillation_never_distilled_trigger, 30,
        "distillation_never_distilled_trigger"
    );
    assert_eq!(
        ab.distillation_legacy_min_messages, 10,
        "distillation_legacy_min_messages"
    );
    assert_eq!(
        ab.distillation_max_backoff_turns, 8,
        "distillation_max_backoff_turns"
    );

    // Competence
    assert!(
        (ab.competence_correction_penalty - 0.05).abs() < f64::EPSILON,
        "competence_correction_penalty"
    );
    assert!(
        (ab.competence_success_bonus - 0.02).abs() < f64::EPSILON,
        "competence_success_bonus"
    );
    assert!(
        (ab.competence_disagreement_penalty - 0.01).abs() < f64::EPSILON,
        "competence_disagreement_penalty"
    );
    assert!(
        (ab.competence_min_score - 0.1).abs() < f64::EPSILON,
        "competence_min_score"
    );
    assert!(
        (ab.competence_max_score - 0.95).abs() < f64::EPSILON,
        "competence_max_score"
    );
    assert!(
        (ab.competence_default_score - 0.5).abs() < f64::EPSILON,
        "competence_default_score"
    );
    assert!(
        (ab.competence_escalation_failure_threshold - 0.30).abs() < f64::EPSILON,
        "competence_escalation_failure_threshold"
    );
    assert_eq!(
        ab.competence_escalation_min_samples, 5,
        "competence_escalation_min_samples"
    );

    // Drift
    assert_eq!(ab.drift_window_size, 20, "drift_window_size");
    assert_eq!(ab.drift_recent_size, 5, "drift_recent_size");
    assert!(
        (ab.drift_deviation_threshold - 2.0).abs() < f64::EPSILON,
        "drift_deviation_threshold"
    );
    assert_eq!(ab.drift_min_samples, 8, "drift_min_samples");

    // Uncertainty
    assert_eq!(
        ab.uncertainty_max_calibration_points, 1_000,
        "uncertainty_max_calibration_points"
    );

    // Skills
    assert_eq!(ab.skills_max_skills, 5, "skills_max_skills");
    assert_eq!(ab.skills_max_context_chars, 200, "skills_max_context_chars");

    // Working state
    assert_eq!(ab.working_state_ttl_secs, 604_800, "working_state_ttl_secs");

    // Planning
    assert_eq!(ab.planning_max_iterations, 10, "planning_max_iterations");
    assert_eq!(
        ab.planning_stuck_history_window, 20,
        "planning_stuck_history_window"
    );
    assert_eq!(
        ab.planning_stuck_repeated_error_threshold, 3,
        "planning_stuck_repeated_error_threshold"
    );
    assert_eq!(
        ab.planning_stuck_same_args_threshold, 3,
        "planning_stuck_same_args_threshold"
    );
    assert_eq!(
        ab.planning_stuck_alternating_threshold, 3,
        "planning_stuck_alternating_threshold"
    );
    assert_eq!(
        ab.planning_stuck_escalating_retry_threshold, 3,
        "planning_stuck_escalating_retry_threshold"
    );

    // Knowledge tuning
    assert_eq!(
        ab.knowledge_instinct_min_observations, 5,
        "knowledge_instinct_min_observations"
    );
    assert!(
        (ab.knowledge_instinct_min_success_rate - 0.80).abs() < f64::EPSILON,
        "knowledge_instinct_min_success_rate"
    );
    assert!(
        (ab.knowledge_instinct_stability_hours - 168.0).abs() < f64::EPSILON,
        "knowledge_instinct_stability_hours"
    );
    assert!(
        (ab.knowledge_surprise_threshold - 2.0).abs() < f64::EPSILON,
        "knowledge_surprise_threshold"
    );
    assert!(
        (ab.knowledge_surprise_ema_alpha - 0.3).abs() < f64::EPSILON,
        "knowledge_surprise_ema_alpha"
    );
    assert_eq!(
        ab.knowledge_rule_min_observations, 5,
        "knowledge_rule_min_observations"
    );
    assert!(
        (ab.knowledge_rule_min_confidence - 0.60).abs() < f64::EPSILON,
        "knowledge_rule_min_confidence"
    );
    assert!(
        (ab.knowledge_dedup_weight_name - 0.4).abs() < f64::EPSILON,
        "knowledge_dedup_weight_name"
    );
    assert!(
        (ab.knowledge_dedup_weight_embed - 0.3).abs() < f64::EPSILON,
        "knowledge_dedup_weight_embed"
    );
    assert!(
        (ab.knowledge_dedup_weight_type - 0.2).abs() < f64::EPSILON,
        "knowledge_dedup_weight_type"
    );
    assert!(
        (ab.knowledge_dedup_weight_alias - 0.1).abs() < f64::EPSILON,
        "knowledge_dedup_weight_alias"
    );
    assert!(
        (ab.knowledge_dedup_jw_threshold - 0.85).abs() < f64::EPSILON,
        "knowledge_dedup_jw_threshold"
    );
    assert!(
        (ab.knowledge_dedup_embed_threshold - 0.80).abs() < f64::EPSILON,
        "knowledge_dedup_embed_threshold"
    );

    // Fact lifecycle
    assert!(
        (ab.fact_active_threshold - 0.7).abs() < f64::EPSILON,
        "fact_active_threshold"
    );
    assert!(
        (ab.fact_fading_threshold - 0.3).abs() < f64::EPSILON,
        "fact_fading_threshold"
    );
    assert!(
        (ab.fact_dormant_threshold - 0.1).abs() < f64::EPSILON,
        "fact_dormant_threshold"
    );

    // Similarity
    assert!(
        (ab.similarity_threshold - 0.85).abs() < f64::EPSILON,
        "similarity_threshold"
    );

    // Tool behavior
    assert_eq!(
        ab.tool_agent_dispatch_max_tasks, 10,
        "tool_agent_dispatch_max_tasks"
    );
    assert_eq!(
        ab.tool_datalog_default_row_limit, 100,
        "tool_datalog_default_row_limit"
    );
    assert!(
        (ab.tool_datalog_default_timeout_secs - 5.0).abs() < f64::EPSILON,
        "tool_datalog_default_timeout_secs"
    );
    assert_eq!(ab.tool_max_image_bytes, 20_971_520, "tool_max_image_bytes");
    assert_eq!(ab.tool_max_pdf_bytes, 33_554_432, "tool_max_pdf_bytes");

    // Corrections
    assert_eq!(
        ab.corrections_max_corrections, 50,
        "corrections_max_corrections"
    );
}
