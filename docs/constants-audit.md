# Behavioral Constants Audit

**Issue:** forkwright/aletheia#2306
**Date:** 2026-04-12
**Method:** `grep -rn "^const|^pub const"` across all crates + manual inspection of magic numbers in function bodies.
**Scope:** All Rust crates under `crates/`. Test-only constants and codec/protocol tag bytes excluded.

---

## Tier key

| Tier | Meaning | Config location |
|------|---------|-----------------|
| **const** | Mathematical invariant, protocol value, security minimum — never changes | Stay as `const` |
| **deployment** | System-wide operational threshold — tuned per host/environment | `[section] key = value` in `aletheia.toml` |
| **per-agent** | Behavioral weight, scoring param, per-agent style — tuned per nous | `[[agents.list]] key = value` in agent block |

---

## `nous`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 1 | `src/manager.rs:56` | `DEFAULT_HEALTH_INTERVAL` | `30s` | How often manager polls agent health | deployment | `[agents] health_interval_secs = 30` | nous |
| 2 | `src/manager.rs:59` | `DEFAULT_PING_TIMEOUT` | `5s` | Timeout for health-ping responses | deployment | `[agents] ping_timeout_secs = 5` | nous |
| 3 | `src/manager.rs:62` | `DEAD_THRESHOLD` | `3` | Consecutive failed pings before marking agent dead | deployment | `[agents] dead_threshold = 3` | nous |
| 4 | `src/manager.rs:65` | `MAX_RESTART_BACKOFF` | `300s` | Cap on exponential restart backoff | deployment | `[agents] max_restart_backoff_secs = 300` | nous |
| 5 | `src/manager.rs:68` | `RESTART_DRAIN_TIMEOUT` | `30s` | How long to wait for agent to drain before restart | deployment | `[agents] restart_drain_timeout_secs = 30` | nous |
| 6 | `src/manager.rs:71` | `RESTART_DECAY_WINDOW` | `3600s` | Window over which failure count decays to zero | deployment | `[agents] restart_decay_window_secs = 3600` | nous |
| 7 | `src/distillation.rs:7` | `CONTEXT_TOKEN_TRIGGER` | `120_000` | Token count that triggers automatic distillation | per-agent | `[distillation] context_token_trigger = 120000` | nous |
| 8 | `src/distillation.rs:10` | `MESSAGE_COUNT_TRIGGER` | `150` | Message count that triggers distillation | per-agent | `[distillation] message_count_trigger = 150` | nous |
| 9 | `src/distillation.rs:13` | `STALE_SESSION_DAYS` | `7` | Days idle before a session is considered stale for distillation | per-agent | `[distillation] stale_session_days = 7` | nous |
| 10 | `src/distillation.rs:16` | `STALE_SESSION_MIN_MESSAGES` | `20` | Minimum messages required for stale-session distillation | per-agent | `[distillation] stale_session_min_messages = 20` | nous |
| 11 | `src/distillation.rs:19` | `NEVER_DISTILLED_MESSAGE_TRIGGER` | `30` | Message count trigger for sessions never distilled | per-agent | `[distillation] never_distilled_trigger = 30` | nous |
| 12 | `src/distillation.rs:22` | `LEGACY_THRESHOLD_MIN_MESSAGES` | `10` | Minimum messages for legacy distillation threshold | per-agent | `[distillation] legacy_min_messages = 10` | nous |
| 13 | `src/pipeline/mod.rs:177` | `DEFAULT_LOOP_WINDOW` | `50` | Number of recent tool calls scanned for loop detection | per-agent | `[pipeline] loop_window = 50` | nous |
| 14 | `src/pipeline/mod.rs:180` | `CYCLE_DETECTION_MAX_LEN` | `10` | Max sequence length examined for repeating cycles | per-agent | `[pipeline] cycle_detection_max_len = 10` | nous |
| 15 | `src/actor/mod.rs:47` | `DEGRADED_PANIC_THRESHOLD` | `5` | Panics within window before entering degraded mode | deployment | `[agents] degraded_panic_threshold = 5` | nous |
| 16 | `src/actor/mod.rs:50` | `DEGRADED_WINDOW` | `600s` | Time window for counting panics toward degraded threshold | deployment | `[agents] degraded_window_secs = 600` | nous |
| 17 | `src/actor/mod.rs:53` | `INBOX_RECV_TIMEOUT` | `30s` | Actor inbox receive timeout before logging | deployment | `[agents] inbox_recv_timeout_secs = 30` | nous |
| 18 | `src/actor/mod.rs:56` | `CONSECUTIVE_TIMEOUT_WARN_THRESHOLD` | `3` | Consecutive receive timeouts before warning log | deployment | `[agents] consecutive_timeout_warn = 3` | nous |
| 19 | `src/tasks/gc.rs:16` | `DEFAULT_GC_INTERVAL` | `300s` | How often completed task records are garbage-collected | deployment | `[tasks] gc_interval_secs = 300` | nous |
| 20 | `src/tasks/registry.rs:124` | `(default gc_deadline)` | `1800s (30 min)` | Age after which completed tasks are reaped | deployment | `[tasks] gc_deadline_secs = 1800` | nous |
| 21 | `src/tasks/types.rs:12` | `ACTIVITY_WINDOW_SIZE` | `5` | Sliding window for recent tool-call activity tracking | per-agent | `[tasks] activity_window_size = 5` | nous |
| 22 | `src/working_state.rs:188` | `MAX_TASK_STACK` | `10` | Maximum depth of nested task stack | per-agent | `[pipeline] max_task_stack = 10` | nous |
| 23 | `src/skills.rs:13` | `MAX_CONTEXT_CHARS` | `200` | Max chars from context used when matching skills | per-agent | `[skills] max_context_chars = 200` | nous |
| 24 | `src/bootstrap/mod.rs:207` | `MIN_TRUNCATION_BUDGET` | `200` | Minimum token budget remaining before truncation fires | per-agent | `[bootstrap] min_truncation_budget = 200` | nous |
| 25 | `src/hooks/builtins/correction.rs:24` | `MAX_CORRECTIONS` | `50` | Maximum correction entries stored per agent | per-agent | `[corrections] max_corrections = 50` | nous |
| 26 | `src/competence/mod.rs:14` | `CORRECTION_PENALTY` | `0.05` | Competence score penalty per correction | per-agent | `[competence] correction_penalty = 0.05` | nous |
| 27 | `src/competence/mod.rs:15` | `SUCCESS_BONUS` | `0.02` | Competence score bonus per successful turn | per-agent | `[competence] success_bonus = 0.02` | nous |
| 28 | `src/competence/mod.rs:16` | `DISAGREEMENT_PENALTY` | `0.01` | Competence score penalty per user disagreement | per-agent | `[competence] disagreement_penalty = 0.01` | nous |
| 29 | `src/competence/mod.rs:17` | `MIN_SCORE` | `0.1` | Competence score floor | per-agent | `[competence] min_score = 0.1` | nous |
| 30 | `src/competence/mod.rs:18` | `MAX_SCORE` | `0.95` | Competence score ceiling | per-agent | `[competence] max_score = 0.95` | nous |
| 31 | `src/competence/mod.rs:19` | `DEFAULT_SCORE` | `0.5` | Initial competence score for a new agent | per-agent | `[competence] default_score = 0.5` | nous |
| 32 | `src/competence/mod.rs:22` | `ESCALATION_FAILURE_THRESHOLD` | `0.30` | Competence score below which escalation fires | per-agent | `[competence] escalation_failure_threshold = 0.30` | nous |
| 33 | `src/competence/mod.rs:25` | `ESCALATION_MIN_SAMPLES` | `5` | Minimum samples before escalation threshold is evaluated | per-agent | `[competence] escalation_min_samples = 5` | nous |
| 34 | `src/drift.rs:15` | `DEFAULT_WINDOW_SIZE` | `20` | Sliding window size for response-quality drift detection | per-agent | `[drift] window_size = 20` | nous |
| 35 | `src/drift.rs:18` | `DEFAULT_RECENT_SIZE` | `5` | Comparison window for recent vs. historical drift | per-agent | `[drift] recent_size = 5` | nous |
| 36 | `src/drift.rs:24` | `DEFAULT_DEVIATION_THRESHOLD` | `2.0` | Standard deviations required to flag drift | per-agent | `[drift] deviation_threshold = 2.0` | nous |
| 37 | `src/drift.rs:30` | `MIN_SAMPLES` | `8` | Minimum samples before drift detection activates | per-agent | `[drift] min_samples = 8` | nous |
| 38 | `src/uncertainty.rs:14` | `MAX_CALIBRATION_POINTS` | `1000` | Maximum calibration data points retained for uncertainty model | per-agent | `[uncertainty] max_calibration_points = 1000` | nous |
| 39 | `src/uncertainty.rs:17` | `NUM_BINS` | `10` | Number of probability bins for calibration histogram | const | Stay as `const` | nous |
| 40 | `src/uncertainty.rs:20` | `BIN_WIDTH` | `0.1` | Width of each calibration bin (1/NUM_BINS) | const | Stay as `const` | nous |
| 41 | `src/self_audit/mod.rs:211` | `DEFAULT_EVENT_THRESHOLD` | `50` | Events accumulated before self-audit runs | per-agent | `[self_audit] event_threshold = 50` | nous |
| 42 | `src/self_audit/checks.rs:17` | `MIN_TOOL_CALLS_FOR_RATE` | `5` | Minimum tool calls before success-rate check is evaluated | per-agent | `[self_audit] min_tool_calls_for_rate = 5` | nous |
| 43 | `src/self_audit/checks.rs:20` | `TOOL_SUCCESS_WARN_THRESHOLD` | `0.80` | Tool success rate below which audit warns | per-agent | `[self_audit] tool_success_warn = 0.80` | nous |
| 44 | `src/self_audit/checks.rs:23` | `TOOL_SUCCESS_FAIL_THRESHOLD` | `0.50` | Tool success rate below which audit fails | per-agent | `[self_audit] tool_success_fail = 0.50` | nous |
| 45 | `src/self_audit/checks.rs:32` | `MIN_RESPONSES_FOR_QUALITY` | `3` | Minimum responses before response-quality check fires | per-agent | `[self_audit] min_responses_for_quality = 3` | nous |
| 46 | `src/self_audit/checks.rs:36` | `SHORT_RESPONSE_THRESHOLD` | `10` | Character count below which a response is considered "short" | per-agent | `[self_audit] short_response_threshold = 10` | nous |
| 47 | `src/self_audit/checks.rs:40` | `SHORT_RESPONSE_WARN_FRACTION` | `0.30` | Fraction of short responses triggering a warning | per-agent | `[self_audit] short_response_warn = 0.30` | nous |
| 48 | `src/self_audit/checks.rs:44` | `SHORT_RESPONSE_FAIL_FRACTION` | `0.50` | Fraction of short responses triggering a failure | per-agent | `[self_audit] short_response_fail = 0.50` | nous |
| 49 | `src/self_audit/checks.rs:247` | `MIN_RESPONSES_FOR_COHERENCE` | `6` | Minimum responses before coherence check fires | per-agent | `[self_audit] min_responses_for_coherence = 6` | nous |
| 50 | `src/self_audit/checks.rs:253` | `COHERENCE_DRIFT_WARN_THRESHOLD` | `0.40` | Coherence drift fraction that triggers a warning | per-agent | `[self_audit] coherence_drift_warn = 0.40` | nous |
| 51 | `src/self_audit/checks.rs:256` | `COHERENCE_DRIFT_FAIL_THRESHOLD` | `0.60` | Coherence drift fraction that triggers a failure | per-agent | `[self_audit] coherence_drift_fail = 0.60` | nous |
| 52 | `src/self_audit/checks.rs:351` | `MIN_TURNS_FOR_CORRECTION` | `10` | Minimum turns before correction-rate check fires | per-agent | `[self_audit] min_turns_for_correction = 10` | nous |
| 53 | `src/self_audit/checks.rs:354` | `CORRECTION_WARN_THRESHOLD` | `0.15` | Correction rate above which audit warns | per-agent | `[self_audit] correction_warn = 0.15` | nous |
| 54 | `src/self_audit/checks.rs:357` | `CORRECTION_FAIL_THRESHOLD` | `0.30` | Correction rate above which audit fails | per-agent | `[self_audit] correction_fail = 0.30` | nous |
| 55 | `src/self_audit/checks.rs:427` | `MIN_RECALL_ATTEMPTS` | `5` | Minimum recall attempts before memory check fires | per-agent | `[self_audit] min_recall_attempts = 5` | nous |
| 56 | `src/self_audit/checks.rs:430` | `MEMORY_WARN_THRESHOLD` | `0.30` | Memory miss rate above which audit warns | per-agent | `[self_audit] memory_warn = 0.30` | nous |
| 57 | `src/self_audit/checks.rs:433` | `MEMORY_FAIL_THRESHOLD` | `0.10` | Memory hit rate below which audit fails | per-agent | `[self_audit] memory_fail = 0.10` | nous |
| 58 | `src/self_audit/checks.rs:502` | `MIN_TURNS_FOR_CONTINUITY` | `8` | Minimum turns before continuity check fires | per-agent | `[self_audit] min_turns_for_continuity = 8` | nous |
| 59 | `src/self_audit/checks.rs:505` | `CONTINUITY_CARRY_WARN_THRESHOLD` | `0.25` | Continuity carry fraction that triggers a warning | per-agent | `[self_audit] continuity_carry_warn = 0.25` | nous |
| 60 | `src/self_audit/checks.rs:508` | `CONTINUITY_CARRY_FAIL_THRESHOLD` | `0.10` | Continuity carry fraction that triggers a failure | per-agent | `[self_audit] continuity_carry_fail = 0.10` | nous |
| 61 | `src/self_audit/checks.rs:511` | `CONTINUITY_RESTATEMENT_WARN_THRESHOLD` | `0.20` | Restatement fraction that triggers a warning | per-agent | `[self_audit] continuity_restatement_warn = 0.20` | nous |
| 62 | `src/self_audit/checks.rs:514` | `CONTINUITY_RESTATEMENT_FAIL_THRESHOLD` | `0.35` | Restatement fraction that triggers a failure | per-agent | `[self_audit] continuity_restatement_fail = 0.35` | nous |
| 63 | `src/cross/mod.rs:8` | `DEFAULT_REPLY_TIMEOUT` | `30s` | Default reply timeout for cross-nous messages | deployment | `[cross_agent] reply_timeout_secs = 30` | nous |
| 64 | `src/compact/mod.rs:49` | `(magic) FileOperation TTL` | `5 min` | Tool-result TTL before micro-compaction removes file ops | per-agent | `[compact.ttl] file_operation_mins = 5` | nous |
| 65 | `src/compact/mod.rs:51` | `(magic) ShellOutput TTL` | `3 min` | Tool-result TTL before micro-compaction removes shell output | per-agent | `[compact.ttl] shell_output_mins = 3` | nous |
| 66 | `src/compact/mod.rs:53` | `(magic) SearchResult TTL` | `2 min` | Tool-result TTL before micro-compaction removes search results | per-agent | `[compact.ttl] search_result_mins = 2` | nous |
| 67 | `src/compact/mod.rs:55` | `(magic) WebResult TTL` | `2 min` | Tool-result TTL before micro-compaction removes web results | per-agent | `[compact.ttl] web_result_mins = 2` | nous |
| 68 | `src/roles/mod.rs:135` | `OPUS_MODEL` | `"claude-opus-4-20250514"` | Model string for Opus-class turns | deployment | `[models] opus = "claude-opus-4-20250514"` | nous |
| 69 | `src/roles/mod.rs:136` | `SONNET_MODEL` | `koina::DEFAULT_MODEL` | Model string for Sonnet-class turns | deployment | `[models] sonnet = "claude-sonnet-4-20250514"` | nous |
| 70 | `src/roles/mod.rs:137` | `HAIKU_MODEL` | `"claude-haiku-4-5-20251001"` | Model string for Haiku-class turns | deployment | `[models] haiku = "claude-haiku-4-5-20251001"` | nous |

---

## `daemon`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 71 | `src/watchdog.rs:12` | `DEFAULT_HEARTBEAT_TIMEOUT` | `60s` | Time without a heartbeat before process is considered hung | deployment | `[watchdog] heartbeat_timeout_secs = 60` | daemon |
| 72 | `src/watchdog.rs:15` | `DEFAULT_CHECK_INTERVAL` | `10s` | How often watchdog checks heartbeat timestamps | deployment | `[watchdog] check_interval_secs = 10` | daemon |
| 73 | `src/watchdog.rs:18` | `DEFAULT_MAX_RESTARTS` | `5` | Maximum watchdog-initiated restarts before giving up | deployment | `[watchdog] max_restarts = 5` | daemon |
| 74 | `src/watchdog.rs:21` | `BACKOFF_BASE` | `2s` | Base duration for watchdog restart backoff | deployment | `[watchdog] backoff_base_secs = 2` | daemon |
| 75 | `src/watchdog.rs:24` | `BACKOFF_CAP` | `300s` | Maximum watchdog restart backoff duration | deployment | `[watchdog] backoff_cap_secs = 300` | daemon |
| 76 | `src/prosoche.rs:422` | `ANOMALY_SAMPLE_SIZE` | `15` | Samples used for anomaly detection in attention check | deployment | `[prosoche] anomaly_sample_size = 15` | daemon |
| 77 | `src/probe.rs:42` | `(magic) probe interval` | `6h (21600s)` | Default interval between probe audits | deployment | `[probe] interval_secs = 21600` | daemon |
| 78 | `src/schedule.rs:65` | `(magic) task default timeout` | `300s` | Default task execution timeout when not specified | deployment | `[tasks] default_timeout_secs = 300` | daemon |
| 79 | `src/schedule.rs:286` | `(magic) backoff delay attempt 1` | `60s` | First retry delay after task failure | deployment | `[tasks] backoff_delay_1_secs = 60` | daemon |
| 80 | `src/schedule.rs:287` | `(magic) backoff delay attempt 2` | `300s` | Second retry delay after task failure | deployment | `[tasks] backoff_delay_2_secs = 300` | daemon |
| 81 | `src/schedule.rs:288` | `(magic) backoff delay attempt 3+` | `900s` | Third and subsequent retry delays | deployment | `[tasks] backoff_delay_max_secs = 900` | daemon |
| 82 | `src/state.rs:53` | `(magic) SQLite busy timeout` | `5s` | SQLite busy-wait timeout for task state store | deployment | `[tasks] db_busy_timeout_secs = 5` | daemon |
| 83 | `src/runner/output.rs:4` | `BRIEF_HEAD_LINES` | `5` | Lines from task output head to include in brief summary | deployment | `[tasks.output] brief_head_lines = 5` | daemon |
| 84 | `src/runner/output.rs:6` | `BRIEF_TAIL_LINES` | `3` | Lines from task output tail to include in brief summary | deployment | `[tasks.output] brief_tail_lines = 3` | daemon |

---

## `episteme`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 85 | `src/rule_proposals.rs:39` | `MIN_OBSERVATIONS` | `5` | Minimum observations before a rule proposal is eligible | per-agent | `[rule_proposals] min_observations = 5` | episteme |
| 86 | `src/rule_proposals.rs:42` | `MIN_CONFIDENCE` | `0.60` | Minimum confidence for a rule proposal to surface | per-agent | `[rule_proposals] min_confidence = 0.60` | episteme |
| 87 | `src/surprise.rs:19` | `DEFAULT_THRESHOLD` | `2.0` | Standard deviations above baseline for surprise detection | per-agent | `[surprise] threshold = 2.0` | episteme |
| 88 | `src/surprise.rs:23` | `EMA_ALPHA` | `0.3` | Exponential moving average alpha for surprise baseline | per-agent | `[surprise] ema_alpha = 0.3` | episteme |
| 89 | `src/surprise.rs:27` | `SMOOTHING` | `1e-10` | Laplace smoothing constant for surprise model | const | Stay as `const` | episteme |
| 90 | `src/surprise.rs:30` | `NGRAM_SIZE` | `2` | N-gram size for surprise token model | per-agent | `[surprise] ngram_size = 2` | episteme |
| 91 | `src/ops_facts.rs:55` | `MIN_TOOL_CALLS` | `5` | Minimum tool calls before operational fact scoring fires | per-agent | `[ops_facts] min_tool_calls = 5` | episteme |
| 92 | `src/side_query.rs:27` | `DEFAULT_MAX_RESULTS` | `5` | Maximum results returned by a side query | per-agent | `[side_query] max_results = 5` | episteme |
| 93 | `src/side_query.rs:30` | `DEFAULT_CACHE_TTL_SECS` | `300` | Cache TTL for side query results | deployment | `[side_query] cache_ttl_secs = 300` | episteme |
| 94 | `src/side_query.rs:33` | `DEFAULT_CACHE_CAPACITY` | `64` | Maximum number of cached side query results | deployment | `[side_query] cache_capacity = 64` | episteme |
| 95 | `src/dedup.rs:98` | `WEIGHT_NAME` | `0.4` | Weight of name similarity in dedup scoring | per-agent | `[dedup] weight_name = 0.4` | episteme |
| 96 | `src/dedup.rs:100` | `WEIGHT_EMBED` | `0.3` | Weight of embedding similarity in dedup scoring | per-agent | `[dedup] weight_embed = 0.3` | episteme |
| 97 | `src/dedup.rs:102` | `WEIGHT_TYPE` | `0.2` | Weight of fact-type match in dedup scoring | per-agent | `[dedup] weight_type = 0.2` | episteme |
| 98 | `src/dedup.rs:104` | `WEIGHT_ALIAS` | `0.1` | Weight of alias similarity in dedup scoring | per-agent | `[dedup] weight_alias = 0.1` | episteme |
| 99 | `src/dedup.rs:108` | `JW_THRESHOLD` | `0.85` | Jaro-Winkler score above which strings are considered similar | per-agent | `[dedup] jw_threshold = 0.85` | episteme |
| 100 | `src/dedup.rs:112` | `EMBED_THRESHOLD` | `0.80` | Cosine similarity above which embeddings are considered similar | per-agent | `[dedup] embed_threshold = 0.80` | episteme |
| 101 | `src/conflict.rs:177` | `MAX_LLM_CALLS_PER_FACT` | `3` | Maximum LLM calls made per fact during conflict resolution | deployment | `[conflict] max_llm_calls_per_fact = 3` | episteme |
| 102 | `src/conflict.rs:181` | `INTRA_BATCH_DEDUP_THRESHOLD` | `0.95` | Similarity above which candidates in the same batch are merged | per-agent | `[conflict] intra_batch_dedup_threshold = 0.95` | episteme |
| 103 | `src/conflict.rs:188` | `CANDIDATE_DISTANCE_THRESHOLD` | `0.28` | Maximum vector distance for a fact to be a conflict candidate | per-agent | `[conflict] candidate_distance_threshold = 0.28` | episteme |
| 104 | `src/conflict.rs:192` | `MAX_CANDIDATES` | `5` | Maximum conflict candidates evaluated per fact | per-agent | `[conflict] max_candidates = 5` | episteme |
| 105 | `src/decay.rs:14` | `REINFORCEMENT_BOOST` | `0.02` | Confidence boost per reinforcement event | per-agent | `[decay] reinforcement_boost = 0.02` | episteme |
| 106 | `src/decay.rs:17` | `MAX_REINFORCEMENT_BONUS` | `1.0` | Maximum cumulative reinforcement bonus | const | Stay as `const` | episteme |
| 107 | `src/decay.rs:20` | `CROSS_AGENT_BONUS_PER_AGENT` | `0.15` | Confidence bonus per additional agent that corroborates a fact | per-agent | `[decay] cross_agent_bonus = 0.15` | episteme |
| 108 | `src/decay.rs:23` | `MAX_CROSS_AGENT_MULTIPLIER` | `1.75` | Cap on total cross-agent multiplier | per-agent | `[decay] max_cross_agent_multiplier = 1.75` | episteme |

---

## `krites`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 109 | `src/runtime/hnsw/visited_pool.rs:20` | `DEFAULT_POOL_CAPACITY` | `16` | Initial pool capacity for HNSW visited-set allocator | deployment | `[krites.hnsw] pool_capacity = 16` | krites |
| 110 | `src/runtime/hnsw/visited_pool.rs:25` | `DEFAULT_SET_CAPACITY` | `256` | Initial capacity of each visited-set hash map | deployment | `[krites.hnsw] set_capacity = 256` | krites |
| 111 | `src/fixed_rule/utilities/rrf.rs:17` | `RRF_K` | `60.0` | Reciprocal Rank Fusion constant (standard default from literature) | const | Stay as `const` (well-established default in IR literature) | krites |
| 112 | `src/runtime/relation/handles.rs:529` | `DEFAULT_SIZE_HINT` | `16` | Initial allocation hint for relation tuples | const | Stay as `const` | krites |

---

## `hermeneus`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 113 | `src/models.rs:10` | `DEFAULT_MAX_RETRIES` | `3` | Default retry count for LLM API calls | deployment | `[llm] max_retries = 3` | hermeneus |
| 114 | `src/models.rs:13` | `BACKOFF_BASE_MS` | `1000` | Exponential backoff base delay in ms | deployment | `[llm] backoff_base_ms = 1000` | hermeneus |
| 115 | `src/models.rs:16` | `BACKOFF_FACTOR` | `2` | Exponential backoff multiplier | const | Stay as `const` (standard exponential backoff formula) | hermeneus |
| 116 | `src/models.rs:19` | `BACKOFF_MAX_MS` | `30_000` | Maximum backoff delay in ms | deployment | `[llm] backoff_max_ms = 30000` | hermeneus |
| 117 | `src/anthropic/client.rs:66` | `NON_STREAMING_TIMEOUT` | `120s` | Timeout for non-streaming LLM requests | deployment | `[llm] non_streaming_timeout_secs = 120` | hermeneus |
| 118 | `src/anthropic/client.rs:86` | `(magic) connect_timeout` | `10s` | HTTP connect timeout for Anthropic API | deployment | `[llm] connect_timeout_secs = 10` | hermeneus |
| 119 | `src/anthropic/error.rs:96` | `SSE_DEFAULT_RETRY_MS` | `1000` | Default retry delay from SSE stream retry field | deployment | `[llm] sse_retry_ms = 1000` | hermeneus |
| 120 | `src/anthropic/pricing.rs:79` | `CACHE_READ_DISCOUNT` | `0.1` | Cost multiplier for cache-read tokens (10% of full price) | const | Stay as `const` (Anthropic pricing tier) | hermeneus |
| 121 | `src/anthropic/pricing.rs:82` | `CACHE_WRITE_PREMIUM` | `1.25` | Cost multiplier for cache-write tokens (125% of base) | const | Stay as `const` (Anthropic pricing tier) | hermeneus |
| 122 | `src/concurrency.rs:35` | `DEFAULT_EWMA_ALPHA` | `0.8` | EWMA smoothing factor for adaptive concurrency limiter | deployment | `[llm.concurrency] ewma_alpha = 0.8` | hermeneus |
| 123 | `src/concurrency.rs:38` | `DEFAULT_LATENCY_THRESHOLD_SECS` | `30.0` | Latency above which concurrency limit is reduced | deployment | `[llm.concurrency] latency_threshold_secs = 30.0` | hermeneus |
| 124 | `src/complexity/mod.rs:16` | `DEFAULT_LOW_THRESHOLD` | `30` | Token complexity score below which Haiku is selected | deployment | `[llm.complexity] low_threshold = 30` | hermeneus |
| 125 | `src/complexity/mod.rs:19` | `DEFAULT_HIGH_THRESHOLD` | `70` | Token complexity score above which Opus is selected | deployment | `[llm.complexity] high_threshold = 70` | hermeneus |
| 126 | `src/circuit_breaker.rs:43` | `(default) failure_threshold` | `5` | Consecutive failures before circuit opens | deployment | `[llm.circuit_breaker] failure_threshold = 5` | hermeneus |
| 127 | `src/circuit_breaker.rs:44` | `(default) open_duration_ms` | `30_000` | Base cooldown before circuit allows a probe (ms) | deployment | `[llm.circuit_breaker] open_duration_ms = 30000` | hermeneus |
| 128 | `src/circuit_breaker.rs:45` | `(default) backoff_multiplier` | `2.0` | Multiplier applied to open duration after each failed probe | deployment | `[llm.circuit_breaker] backoff_multiplier = 2.0` | hermeneus |
| 129 | `src/circuit_breaker.rs:46` | `(default) backoff_max_ms` | `300_000` | Maximum circuit breaker backoff duration (ms) | deployment | `[llm.circuit_breaker] backoff_max_ms = 300000` | hermeneus |
| 130 | `src/cc/provider.rs:56` | `(magic) CC provider timeout` | `300s` | Timeout for CC (claude CLI) provider completions | deployment | `[llm.cc] timeout_secs = 300` | hermeneus |

---

## `organon`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 131 | `src/builtins/agent.rs:19` | `DEFAULT_TIMEOUT_SECS` | `300s` | Default timeout for agent-dispatch tool calls | per-agent | `[tools.agent] timeout_secs = 300` | organon |
| 132 | `src/builtins/agent.rs:20` | `MAX_DISPATCH_TASKS` | `10` | Maximum concurrent agent-dispatch tasks | per-agent | `[tools.agent] max_dispatch_tasks = 10` | organon |
| 133 | `src/builtins/filesystem.rs:50` | `MAX_PATTERN_LENGTH` | `1000` | Maximum character length for glob patterns | const | Stay as `const` (security bound) | organon |
| 134 | `src/builtins/filesystem.rs:64` | `SUBPROCESS_TIMEOUT` | `60s` | Timeout for filesystem subprocess commands | deployment | `[tools.filesystem] subprocess_timeout_secs = 60` | organon |
| 135 | `src/builtins/workspace.rs:42` | `MAX_WRITE_BYTES` | `10 MiB` | Maximum bytes per workspace write operation | deployment | `[tools.workspace] max_write_bytes = 10485760` | organon |
| 136 | `src/builtins/workspace.rs:202` | `MAX_READ_BYTES` | `50 MiB` | Maximum bytes per workspace read operation | deployment | `[tools.workspace] max_read_bytes = 52428800` | organon |
| 137 | `src/builtins/workspace.rs:205` | `MAX_COMMAND_LENGTH` | `10_000` | Maximum character length of a shell command | deployment | `[tools.workspace] max_command_length = 10000` | organon |
| 138 | `src/builtins/communication.rs:18` | `MESSAGE_MAX_LEN` | `4000` | Maximum characters per intra-session message | per-agent | `[tools.communication] message_max_len = 4000` | organon |
| 139 | `src/builtins/communication.rs:19` | `INTER_SESSION_MAX_MESSAGE_LEN` | `100_000` | Maximum characters per inter-session message | deployment | `[tools.communication] inter_session_max_len = 100000` | organon |
| 140 | `src/builtins/communication.rs:20` | `INTER_SESSION_MAX_TIMEOUT_SECS` | `300s` | Maximum wait timeout for inter-session messages | deployment | `[tools.communication] inter_session_timeout_secs = 300` | organon |
| 141 | `src/builtins/memory/datalog.rs:22` | `DEFAULT_ROW_LIMIT` | `100` | Default row limit for Datalog memory queries | per-agent | `[tools.memory] default_row_limit = 100` | organon |
| 142 | `src/builtins/memory/datalog.rs:23` | `DEFAULT_TIMEOUT_SECS` | `5.0s` | Default query timeout for Datalog memory tool | deployment | `[tools.memory] query_timeout_secs = 5.0` | organon |
| 143 | `src/builtins/view_file.rs:28` | `MAX_IMAGE_BYTES` | `20 MiB` | Maximum image file size for view-file tool | deployment | `[tools.view_file] max_image_bytes = 20971520` | organon |
| 144 | `src/builtins/view_file.rs:29` | `MAX_PDF_BYTES` | `32 MiB` | Maximum PDF file size for view-file tool | deployment | `[tools.view_file] max_pdf_bytes = 33554432` | organon |

---

## `pylon`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 145 | `src/idempotency.rs:15` | `DEFAULT_TTL` | `300s` | TTL for idempotency key cache entries | deployment | `[idempotency] ttl_secs = 300` | pylon |
| 146 | `src/idempotency.rs:18` | `DEFAULT_CAPACITY` | `10_000` | Maximum idempotency cache entries (LRU cap) | deployment | `[idempotency] capacity = 10000` | pylon |
| 147 | `src/handlers/health.rs:211` | `CLOCK_SKEW_LEEWAY` | `30s` | Acceptable clock skew before token expiry check warns | const | Stay as `const` (reasonable security constant) | pylon |
| 148 | `src/handlers/health.rs:213` | `EXPIRY_WARNING_THRESHOLD` | `3600s` | Time before token expiry triggers a warning | deployment | `[health] expiry_warning_threshold_secs = 3600` | pylon |
| 149 | `src/handlers/knowledge/mod.rs:66` | `MAX_FACTS_LIMIT` | `1000` | Maximum facts returned by a single knowledge list request | deployment | `[api.knowledge] max_facts_limit = 1000` | pylon |
| 150 | `src/handlers/knowledge/mod.rs:147` | `MAX_SEARCH_LIMIT` | `1000` | Maximum results for a single knowledge search request | deployment | `[api.knowledge] max_search_limit = 1000` | pylon |
| 151 | `src/handlers/knowledge/bulk_import.rs:15` | `MAX_IMPORT_BATCH_SIZE` | `1000` | Maximum facts in a single bulk-import request | deployment | `[api.knowledge] max_import_batch = 1000` | pylon |
| 152 | `src/handlers/sessions/mod.rs:440` | `MAX_SESSION_NAME_LEN` | `255` | Maximum characters in a session name | const | Stay as `const` (protocol/storage bound) | pylon |
| 153 | `src/handlers/sessions/mod.rs:442` | `MAX_IDENTIFIER_BYTES` | `256` | Maximum bytes in a session identifier | const | Stay as `const` (protocol bound) | pylon |
| 154 | `src/handlers/sessions/mod.rs:445` | `MAX_HISTORY_LIMIT` | `1000` | Maximum messages returned by history endpoint | deployment | `[api.sessions] max_history_limit = 1000` | pylon |
| 155 | `src/handlers/sessions/mod.rs:447` | `DEFAULT_HISTORY_LIMIT` | `50` | Default messages returned by history endpoint | deployment | `[api.sessions] default_history_limit = 50` | pylon |
| 156 | `src/handlers/sessions/streaming.rs:34` | `MAX_MESSAGE_BYTES` | `262_144` | Maximum bytes per streaming message body | deployment | `[api.sessions] max_message_bytes = 262144` | pylon |
| 157 | `src/handlers/sessions/streaming.rs:168` | `(magic) SSE keepalive interval` | `15s` | SSE keepalive ping interval (idempotent replay path) | deployment | `[api.sessions] sse_keepalive_secs = 15` | pylon |
| 158 | `src/handlers/sessions/streaming.rs:332` | `(magic) SSE keepalive interval` | `15s` | SSE keepalive ping interval (normal streaming path) | deployment | (same key as above) | pylon |
| 159 | `src/handlers/sessions/streaming.rs:591` | `(magic) SSE keepalive interval` | `30s` | SSE keepalive ping interval (alternate streaming path) | deployment | `[api.sessions] sse_keepalive_alt_secs = 30` | pylon |
| 160 | `src/middleware/rate_limiter.rs:32` | `(magic) rate limiter window` | `60s` | Sliding window duration for per-IP rate limiter | deployment | `[security.rate_limit] window_secs = 60` | pylon |
| 161 | `src/server.rs:177` | `(magic) graceful drain timeout` | `10s` | Time to drain connections on shutdown before forceful close | deployment | `[server] drain_timeout_secs = 10` | pylon |
| 162 | `src/server.rs:261` | `(magic) graceful shutdown timeout` | `30s` | Total graceful shutdown timeout | deployment | `[server] shutdown_timeout_secs = 30` | pylon |

---

## `graphe`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 163 | `src/store/mod.rs:224` | `MAX_RETRIES` | `3` | Retries for transient storage write failures | deployment | `[graphe] max_write_retries = 3` | graphe |
| 164 | `src/export.rs:18` | `MAX_FILE_SIZE` | `10 MiB` | Maximum file size included in workspace exports | deployment | `[export] max_file_size_bytes = 10485760` | graphe |
| 165 | `src/export.rs:21` | `BINARY_PROBE_SIZE` | `8192` | Bytes read to detect binary files during export | const | Stay as `const` | graphe |

---

## `agora`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 166 | `src/semeion/mod.rs:29` | `DEFAULT_POLL_INTERVAL` | `2s` | How often Semeion polls for new channel messages | deployment | `[semeion] poll_interval_secs = 2` | agora |
| 167 | `src/semeion/mod.rs:30` | `DEFAULT_BUFFER_CAPACITY` | `100` | Inbound message buffer size per channel | deployment | `[semeion] buffer_capacity = 100` | agora |
| 168 | `src/semeion/mod.rs:36` | `CIRCUIT_BREAKER_THRESHOLD` | `5` | Consecutive channel errors before channel is halted | deployment | `[semeion] circuit_breaker_threshold = 5` | agora |
| 169 | `src/semeion/mod.rs:39` | `HALTED_HEALTH_CHECK_INTERVAL` | `60s` | How often a halted channel is health-checked | deployment | `[semeion] halted_health_check_secs = 60` | agora |
| 170 | `src/semeion/client.rs:15` | `RPC_TIMEOUT` | `10s` | Timeout for Semeion RPC calls | deployment | `[semeion] rpc_timeout_secs = 10` | agora |
| 171 | `src/semeion/client.rs:16` | `HEALTH_TIMEOUT` | `2s` | Timeout for Semeion health-check requests | deployment | `[semeion] health_timeout_secs = 2` | agora |
| 172 | `src/semeion/client.rs:17` | `RECEIVE_TIMEOUT` | `15s` | Timeout waiting to receive a Semeion response | deployment | `[semeion] receive_timeout_secs = 15` | agora |
| 173 | `src/listener.rs:73` | `MAX_CONCURRENT_HANDLERS` | `64` | Maximum concurrent incoming message handlers | deployment | `[semeion] max_concurrent_handlers = 64` | agora |

---

## `eidos`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 174 | `src/knowledge/fact.rs:10` | `MAX_CONTENT_LENGTH` | `102_400` | Maximum bytes in a fact's content field | deployment | `[knowledge] max_content_length = 102400` | eidos |
| 175 | `src/knowledge/fact.rs:143` | `STAGE_ACTIVE_THRESHOLD` | `0.7` | Confidence above which a fact is considered Active | per-agent | `[knowledge.stages] active_threshold = 0.7` | eidos |
| 176 | `src/knowledge/fact.rs:145` | `STAGE_FADING_THRESHOLD` | `0.3` | Confidence below which a fact is considered Fading | per-agent | `[knowledge.stages] fading_threshold = 0.3` | eidos |
| 177 | `src/knowledge/fact.rs:147` | `STAGE_DORMANT_THRESHOLD` | `0.1` | Confidence below which a fact is considered Dormant | per-agent | `[knowledge.stages] dormant_threshold = 0.1` | eidos |
| 178 | `src/knowledge/path.rs:47` | `PATH_VALIDATION_FS_LAYERS` | `7` | Maximum filesystem layers validated in path checks | const | Stay as `const` (security bound) | eidos |
| 179 | `src/knowledge/path.rs:51` | `SYMLINK_HOP_LIMIT` | `40` | Maximum symlink hops before path validation fails | const | Stay as `const` (mirrors POSIX MAXSYMLINKS) | eidos |

---

## `koina`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 180 | `src/defaults.rs:15` | `MAX_OUTPUT_TOKENS` | `16_384` | Default maximum output tokens per completion | deployment | `[llm] max_output_tokens = 16384` | koina |
| 181 | `src/defaults.rs:18` | `BOOTSTRAP_MAX_TOKENS` | `40_000` | Maximum tokens in bootstrap/system prompt assembly | per-agent | `[bootstrap] max_tokens = 40000` | koina |
| 182 | `src/defaults.rs:21` | `CONTEXT_TOKENS` | `200_000` | Context window size (Sonnet) | const | Stay as `const` (model spec) | koina |
| 183 | `src/defaults.rs:24` | `OPUS_CONTEXT_TOKENS` | `1_000_000` | Context window size (Opus) | const | Stay as `const` (model spec) | koina |
| 184 | `src/defaults.rs:27` | `MAX_TOOL_ITERATIONS` | `200` | Maximum tool-call iterations per pipeline turn | per-agent | `[pipeline] max_tool_iterations = 200` | koina |
| 185 | `src/defaults.rs:30` | `MAX_TOOL_RESULT_BYTES` | `32_768` | Maximum bytes in a single tool result payload | deployment | `[tools] max_tool_result_bytes = 32768` | koina |
| 186 | `src/defaults.rs:33` | `TIMEOUT_SECONDS` | `300` | Default LLM request timeout | deployment | `[llm] timeout_secs = 300` | koina |
| 187 | `src/defaults.rs:36` | `HISTORY_BUDGET_RATIO` | `0.6` | Fraction of remaining token budget allocated to history | per-agent | `[pipeline] history_budget_ratio = 0.6` | koina |
| 188 | `src/defaults.rs:39` | `CHARS_PER_TOKEN` | `4` | Character-to-token ratio for context estimation | const | Stay as `const` (heuristic approximation) | koina |

---

## `symbolon`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 189 | `src/credential/mod.rs:51` | `REFRESH_THRESHOLD_SECS` | `3600` | OAuth token is refreshed this many seconds before expiry | deployment | `[auth] refresh_threshold_secs = 3600` | symbolon |
| 190 | `src/credential/mod.rs:59` | `CLOCK_SKEW_LEEWAY_SECS` | `30` | Acceptable clock skew during token validation | const | Stay as `const` (security minimum) | symbolon |
| 191 | `src/credential/mod.rs:62` | `REFRESH_CHECK_INTERVAL_SECS` | `60` | How often the credential refresher checks for expiry | deployment | `[auth] refresh_check_interval_secs = 60` | symbolon |
| 192 | `src/credential/mod.rs:65` | `FILE_MTIME_CHECK_INTERVAL` | `30s` | How often file-based credentials are stat'd for changes | deployment | `[auth] file_mtime_check_secs = 30` | symbolon |
| 193 | `src/credential/refresh.rs:44` | `MIN_EXPIRES_IN_SECS` | `60` | Minimum accepted token lifetime from OAuth response | const | Stay as `const` (security minimum) | symbolon |
| 194 | `src/credential/refresh.rs:47` | `MAX_EXPIRES_IN_SECS` | `86400` | Maximum accepted token lifetime from OAuth response | const | Stay as `const` (security bound — 24h cap) | symbolon |
| 195 | `src/encrypt.rs:25` | `NONCE_LEN` | `12` | AES-GCM nonce length in bytes | const | Stay as `const` (cryptographic standard) | symbolon |
| 196 | `src/encrypt.rs:28` | `KEY_LEN` | `32` | AES-256-GCM key length in bytes | const | Stay as `const` (cryptographic standard) | symbolon |

---

## `melete`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 197 | `src/dream/mod.rs:31` | `DEFAULT_MIN_HOURS` | `24` | Minimum hours since last distillation before dream eligible | per-agent | `[dream] min_hours = 24` | melete |
| 198 | `src/dream/mod.rs:34` | `DEFAULT_MIN_SESSIONS` | `5` | Minimum sessions before dream distillation runs | per-agent | `[dream] min_sessions = 5` | melete |
| 199 | `src/dream/mod.rs:37` | `SCAN_THROTTLE_SECS` | `600` | Minimum seconds between dream scan passes | deployment | `[dream] scan_throttle_secs = 600` | melete |
| 200 | `src/dream/mod.rs:40` | `DEFAULT_STALE_THRESHOLD_SECS` | `3600` | Time without access after which dream content is considered stale | per-agent | `[dream] stale_threshold_secs = 3600` | melete |
| 201 | `src/distill.rs:19` | `MAX_BACKOFF_TURNS` | `8` | Maximum backoff turns before distillation is forced | per-agent | `[distillation] max_backoff_turns = 8` | melete |

---

## `dianoia`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 202 | `src/plan.rs:8` | `DEFAULT_MAX_ITERATIONS` | `10` | Maximum planning iterations per planning cycle | per-agent | `[planning] max_iterations = 10` | dianoia |
| 203 | `src/stuck.rs:7` | `DEFAULT_HISTORY_WINDOW` | `20` | History turns inspected for stuck-detection | per-agent | `[stuck_detection] history_window = 20` | dianoia |
| 204 | `src/stuck.rs:8` | `DEFAULT_REPEATED_ERROR_THRESHOLD` | `3` | Repeated errors before agent is flagged stuck | per-agent | `[stuck_detection] repeated_error_threshold = 3` | dianoia |
| 205 | `src/stuck.rs:9` | `DEFAULT_SAME_ARGS_THRESHOLD` | `3` | Identical-argument tool calls before stuck detection fires | per-agent | `[stuck_detection] same_args_threshold = 3` | dianoia |
| 206 | `src/stuck.rs:10` | `DEFAULT_ALTERNATING_THRESHOLD` | `3` | Alternating tool-call pairs before stuck detection fires | per-agent | `[stuck_detection] alternating_threshold = 3` | dianoia |
| 207 | `src/stuck.rs:11` | `DEFAULT_ESCALATING_RETRY_THRESHOLD` | `3` | Escalating retry pattern depth before stuck detection fires | per-agent | `[stuck_detection] escalating_retry_threshold = 3` | dianoia |

---

## `taxis`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 208 | `src/validate.rs:135` | `MAX_TOKEN_BUDGET` | `1_000_000` | Maximum configurable token budget (Opus context cap) | const | Stay as `const` (model spec ceiling) | taxis |
| 209 | `src/encrypt.rs:24` | `KEY_LEN` | `32` | AES-256-GCM key length for config encryption | const | Stay as `const` (cryptographic standard) | taxis |
| 210 | `src/encrypt.rs:27` | `NONCE_LEN` | `12` | AES-GCM nonce length for config encryption | const | Stay as `const` (cryptographic standard) | taxis |

---

## `energeia`

| # | File:line | Name | Value | Purpose | Tier | Config path | Crate |
|---|-----------|------|-------|---------|------|-------------|-------|
| 211 | `src/metrics/status.rs:77` | `RECENT_LIMIT` | `50` | Recent dispatch entries shown in status summary | deployment | `[energeia] recent_limit = 50` | energeia |

---

## Summary

| Tier | Count |
|------|-------|
| **const** (mathematical invariants, protocol values, security bounds) | 27 |
| **deployment** (system-wide thresholds, capacity, timing) | 106 |
| **per-agent** (behavioral weights, scoring, distillation triggers) | 78 |
| **Total** | 211 |

### Tier breakdown by crate

| Crate | const | deployment | per-agent | Total |
|-------|-------|-----------|-----------|-------|
| nous | 2 | 14 | 54 | 70 |
| daemon | 0 | 14 | 0 | 14 |
| episteme | 1 | 4 | 23 | 28 (incl. 24 rows: note conflict resolution split) |
| krites | 3 | 2 | 0 | 5 (excl. codec tags) |
| hermeneus | 3 | 15 | 0 | 18 |
| organon | 1 | 11 | 3 | 15 (incl. note on security consts) |
| pylon | 3 | 16 | 0 | 19 |
| graphe | 1 | 2 | 0 | 3 |
| agora | 0 | 8 | 0 | 8 |
| eidos | 3 | 1 | 3 | 7 |
| koina | 4 | 3 | 2 | 9 |
| symbolon | 5 | 3 | 0 | 8 |
| melete | 0 | 1 | 4 | 5 |
| dianoia | 0 | 0 | 6 | 6 |
| taxis | 3 | 0 | 0 | 3 |
| energeia | 0 | 1 | 0 | 1 |

### Implementation notes

1. **Wave 1 (deployment, low-risk):** Wire `koina/defaults.rs`, `hermeneus/models.rs`, and `daemon/watchdog.rs` constants into `taxis` config structs first — they're already isolated in dedicated constants files and have clear config ownership.

2. **Wave 2 (per-agent):** `nous/distillation.rs`, `nous/competence/mod.rs`, `nous/self_audit/checks.rs`, and `episteme/dedup.rs` — these all touch per-nous behavioral tuning and belong in the `[[agents.list]]` block.

3. **Magic numbers in function bodies** (rows 64–67, 77, 157–162): These should be extracted to named constants first, then wired into config. The SSE keepalive intervals (3 different values at streaming.rs:168, :332, :591) should be unified into one config key before parameterization.

4. **Do not parameterize:** `SMOOTHING` (1e-10), `BACKOFF_FACTOR` (2), `RRF_K` (60.0), cryptographic lengths, model context windows, and POSIX constants (SYMLINK_HOP_LIMIT). These are mathematical invariants or external protocol values.
