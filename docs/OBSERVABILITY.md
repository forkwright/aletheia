# Observability for Aletheia operators

Service-level objectives (SLOs), alerting thresholds, and runbook steps for the metrics Prometheus scrapes from Aletheia.

For setup and deployment, see [DEPLOYMENT.md](DEPLOYMENT.md). For day-to-day operational procedures, see [RUNBOOK.md](RUNBOOK.md).

---

## Metric inventory

The `/metrics` endpoint exposes counters, gauges, and histograms from the workspace crates. Metric names use the `aletheia_` prefix.

### HTTP gateway

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_http_requests_total` | Counter | `method`, `path`, `status` | Total HTTP requests by method, normalized path, and status code |
| `aletheia_http_request_duration_seconds` | Histogram | `method`, `path` | Request latency distribution |
| `aletheia_active_sessions` | Gauge | - | Current number of active sessions |
| `aletheia_uptime_seconds` | Gauge | - | Process uptime in seconds |

### LLM providers

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_llm_requests_total` | Counter | `provider`, `status` | Total LLM API requests (`ok` or `error`) |
| `aletheia_llm_request_duration_seconds` | Histogram | `model`, `status` | End-to-end LLM request latency |
| `aletheia_llm_ttft_seconds` | Histogram | `model`, `status` | Time-to-first-token for streaming requests |
| `aletheia_llm_tokens_total` | Counter | `provider`, `direction` | Token consumption (`input` or `output`) |
| `aletheia_llm_cost_usd_total` | Counter | `provider` | Estimated spend in USD |
| `aletheia_llm_cache_tokens_total` | Counter | `provider`, `direction` | Prompt cache tokens (`read` or `write`) |
| `aletheia_llm_circuit_breaker_transitions_total` | Counter | `provider`, `from`, `to` | Provider health state changes (`up`, `degraded`, `down`, `probing`) |
| `aletheia_llm_concurrency_limit` | Gauge | `provider` | Current adaptive concurrency limit |
| `aletheia_llm_concurrency_in_flight` | Gauge | `provider` | In-flight requests |
| `aletheia_llm_concurrency_latency_ewma_seconds` | Gauge | `provider` | EWMA latency estimate used by the limiter |

### Agent pipeline

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_pipeline_turns_total` | Counter | `nous_id`, `provider` | Turns processed per agent and observed provider |
| `aletheia_pipeline_stage_duration_seconds` | Histogram | `nous_id`, `stage` | Per-stage latency (`context`, `recall`, `execute`, etc.) |
| `aletheia_pipeline_errors_total` | Counter | `nous_id`, `stage`, `error_type` | Errors by pipeline stage |
| `aletheia_tool_failures_total` | Counter | `nous_id`, `tool_name` | Tool execution failures |
| `aletheia_stream_events_dropped_total` | Counter | `nous_id`, `reason` | Streaming events dropped (`full` or `disconnected`) |
| `aletheia_nous_inbox_saturation_total` | Counter | `nous_id`, `reason` | Bounded actor inbox saturation (`send_timeout`) |
| `aletheia_nous_background_task_failures_total` | Counter | `nous_id`, `task_type` | Background task failures (distillation, extraction, etc.) |
| `aletheia_cache_read_tokens_total` | Counter | `nous_id` | Prompt cache hits |
| `aletheia_cache_creation_tokens_total` | Counter | `nous_id` | Prompt cache writes |

### Tool execution

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_tool_invocations_total` | Counter | `tool_name`, `status` | Tool calls (`ok` or `error`) |
| `aletheia_tool_duration_seconds` | Histogram | `tool_name` | Tool execution latency |

### Daemon and watchdog

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_watchdog_hung_processes` | Gauge | - | Number of processes currently marked hung |
| `aletheia_watchdog_restarts_total` | Counter | `process_id` | Watchdog-initiated restarts |
| `aletheia_cron_executions_total` | Counter | `task_name`, `status` | Scheduled task runs |
| `aletheia_cron_duration_seconds` | Histogram | `task_name` | Cron task latency |
| `aletheia_background_task_failures_total` | Counter | `nous_id`, `task_type` | Daemon-level background failures |

### Session store

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_sessions_total` | Counter | `session_type` | Sessions created |
| `aletheia_backup_duration_seconds` | Histogram | `status` | Backup duration (`ok` or `error`) |
| `aletheia_backup_last_success_unixtime_seconds` | Gauge | — | `created_at` of the newest valid backup manifest on disk; `0` when none exists |
| `aletheia_backup_enabled` | Gauge | — | Whether periodic whole-instance backups are enabled |
| `aletheia_backup_interval_seconds` | Gauge | — | Configured interval between automatic backups |

> **Note:** `aletheia_backup_duration_seconds` measures *attempts*, not freshness. Failures always record `status="error"`; success records `status="ok"` only when the run produced a backup (`report.backup_path.is_some()`) — skipped backups record nothing. Because the family is process-local, the `status="ok"` series does not exist until this process completes a backup, so it cannot answer "is there a recent backup".
>
> **Freshness comes from the three gauges instead.** They are derived from the backup manifests on disk (`InstanceBackup::latest_backup_time`), published at startup and after every backup outcome — success, skip, and failure — so they survive a restart and are defined even when no backup has ever run. A backup set counts only once it has been atomically renamed into place with a parseable `manifest.json`; an in-progress staging directory or a truncated manifest reads as *no backup*, never as a fresh one. An unreadable backup directory also reports `0`, because at a recovery boundary an unreadable store is not evidence of a good backup.

### Knowledge and embeddings

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_knowledge_facts_total` | Counter | `nous_id` | Facts inserted |
| `aletheia_knowledge_extractions_total` | Counter | `nous_id`, `status` | Extraction operations |
| `aletheia_extraction_quality_total` | Counter | `nous_id`, `producer`, `provider`, `model`, `status`, `reason` | Facts accepted/rejected by reason during extraction refinement |
| `aletheia_extraction_confidence` | Histogram | `nous_id`, `producer`, `provider`, `model`, `status` | Distribution of fact confidence (`extracted`, `accepted`, `rejected`) |
| `aletheia_extraction_confidence_inflation_total` | Counter | `nous_id`, `producer`, `provider`, `model` | Batches where >80% of facts have confidence >= 0.95 |
| `aletheia_extraction_corrections_total` | Counter | `nous_id`, `producer`, `provider`, `model` | Facts flagged as corrections during refinement |
| `aletheia_extraction_contradictions_total` | Counter | `nous_id`, `producer`, `provider`, `model` | Contradictions detected against existing knowledge |
| `aletheia_extraction_conflicts_total` | Counter | `nous_id`, `producer`, `provider`, `model` | All conflicts (contradictions + duplicates) detected |
| `aletheia_knowledge_low_confidence_admissions_total` | Counter | `nous_id`, `threshold` | Facts admitted despite confidence below 0.5 |
| `aletheia_knowledge_admission_total` | Counter | `nous_id`, `fact_type`, `outcome`, `reason` | Admission gate decisions |
| `aletheia_conflict_unclassifiable_total` | Counter | - | Unclassifiable conflict-classifier responses |
| `aletheia_recall_duration_seconds` | Histogram | `nous_id` | Recall scoring latency, per recalling agent |
| `aletheia_embedding_duration_seconds` | Histogram | `provider` | Embedding computation latency |
| `aletheia_memory_health_score` | Gauge | - | Composite memory health score (0.0-1.0), server-computed on each `/metrics` scrape |
| `aletheia_memory_avg_confidence` | Gauge | - | Average confidence across active (non-forgotten, non-superseded) facts |
| `aletheia_memory_orphan_ratio` | Gauge | - | Fraction of entities with no relationship and no fact link |
| `aletheia_memory_staleness_ratio` | Gauge | - | Fraction of active facts unreviewed past the 30-day staleness threshold |

> **Memory health semantics:** these four gauges are server-side counterparts of what `theatron/proskenion`'s Memory Health panel computes client-side from an already-fetched fact/entity list (`crates/theatron/proskenion/src/views/meta/assembly.rs`). Both sides call the identical `koina::memory_health::compute_health_score` formula (0.4 confidence + 0.3 non-orphan + 0.3 non-stale), so the two numbers should track each other; they are computed from independent queries (server scrape vs. client fetch, at different times, over potentially different visibility scopes for a scoped token), not wired point-to-point, so brief divergence during active writes is expected rather than a bug.

> **Quality semantics:** `aletheia_knowledge_facts_total` and `aletheia_knowledge_extractions_total` measure throughput and liveness. The `aletheia_extraction_*_quality` counters measure whether the admitted facts are calibrated, non-redundant, and non-contradictory. A healthy deployment should see stable or falling rejection/empty-extraction rates, a broad confidence distribution rather than a spike at 0.95+, and contradiction rates that are low relative to the volume of new facts.

### Distillation

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_distillation_total` | Counter | `nous_id`, `status` | Distillation runs |
| `aletheia_distillation_duration_seconds` | Histogram | `nous_id` | Distillation latency |
| `aletheia_tokens_saved_total` | Counter | `nous_id` | Tokens saved by compression |

---

## SLOs and thresholds

These thresholds are defaults. Tune them per deployment based on traffic volume, provider latency, and cost sensitivity.

| Objective | Target | Metric basis |
|-----------|--------|--------------|
| Availability | 99.5% over 30 days | `aletheia_http_requests_total` |
| HTTP 5xx rate | < 1% over 5 minutes | `aletheia_http_requests_total{status=~"5.."}` |
| LLM p95 latency | < 30 seconds | `aletheia_llm_request_duration_seconds` |
| LLM TTFT p95 | < 5 seconds | `aletheia_llm_ttft_seconds` |
| Nous inbox saturation | 0 sustained events over 5 minutes | `increase(aletheia_nous_inbox_saturation_total[5m])` |
| Backup freshness | Deployment-defined | `aletheia_backup_last_success_unixtime_seconds` (newest backup manifest on disk; `0` when none) |
| Hung processes | 0 | `aletheia_watchdog_hung_processes` |
| Extraction confidence inflation | < 5% of batches over 10 minutes | `rate(aletheia_extraction_confidence_inflation_total[10m]) / rate(aletheia_knowledge_extractions_total{status="ok"}[10m])` |
| Extraction rejection/empty rate | < 50% of batches over 10 minutes | `rate(aletheia_extraction_quality_total{status="rejected"}[10m]) / rate(aletheia_extraction_quality_total[10m])` |
| Contradiction spike | < 1% of admitted facts over 10 minutes | `rate(aletheia_extraction_contradictions_total[10m]) / rate(aletheia_knowledge_facts_total[10m])` |
| Low-confidence admission rate | < 10% of admitted facts over 10 minutes | `rate(aletheia_knowledge_low_confidence_admissions_total[10m]) / rate(aletheia_knowledge_admission_total{outcome="admitted"}[10m])` |
| Memory health score | >= 0.4 sustained over 30 minutes | `aletheia_memory_health_score` (0.4 matches proskenion's own `health_score_color` red-band threshold) |
| Memory orphan ratio | < 20% sustained over 1 hour | `aletheia_memory_orphan_ratio` |
| Memory staleness ratio | < 15% sustained over 1 hour | `aletheia_memory_staleness_ratio` |

---

## Alert runbook

### AletheiaDown

**What it means:** Prometheus cannot scrape the Aletheia metrics endpoint, or the process has stopped updating its uptime gauge.

**Impact:** Complete service unavailability. All API requests, agent turns, and background tasks stop.

**Steps:**
1. Check process state: `systemctl --user status aletheia`
2. If stopped, start it: `systemctl --user start aletheia`
3. If running but unresponsive, capture logs: `journalctl --user -u aletheia --since "5 minutes ago"`
4. Check for port conflicts: `ss -tlnp | grep 18789`
5. Restart if needed: `systemctl --user restart aletheia`
6. Verify: `curl -sf http://localhost:18789/api/health`

### HighHttpErrorRate

**What it means:** More than 5% of HTTP requests returned a 5xx status over a 5-minute window.

**Impact:** Clients see failures. Agent turns may fail. Streaming connections may drop.

**Steps:**
1. Check logs for panics or unhandled errors: `journalctl --user -u aletheia --priority err..warning --since "10 minutes ago"`
2. Identify the endpoint: filter `aletheia_http_requests_total` by `path` and `status`
3. Check LLM provider health: `curl -sf http://localhost:18789/api/health`
4. If provider errors, verify credentials: `aletheia credential status`
5. If rate-limited, review concurrency settings in `instance/config/aletheia.toml`

### SlowLlmLatency

**What it means:** The 95th percentile of LLM request latency exceeded 30 seconds for 5 minutes.

**Impact:** Slow agent responses. Timeouts in client integrations. Poor user experience.

**Steps:**
1. Check which model is slow: `aletheia_llm_request_duration_seconds` by `model`
2. Review provider status pages for outages
3. Check `aletheia_llm_concurrency_in_flight` and `aletheia_llm_concurrency_limit` for throttling
4. If TTFT is also high, the provider is congested. Consider switching models or providers.
5. If latency spikes for a specific `nous_id`, that agent's context window may be oversized. Archive old sessions.

### LlmCircuitBreakerOpen

**What it means:** A provider's circuit opened — its health tracker transitioned to `down` within the last 5 minutes, after consecutive failures, a rate limit, or an auth failure.

**Impact:** Requests to that provider are failing fast. Fallback or retry logic is active.

**Steps:**
1. Identify the provider from the `provider` label
2. Check provider health and credentials
3. Review `aletheia_llm_requests_total{status="error"}` for error patterns
4. If transient, the circuit auto-recovers: after the cooldown one request is elected as a probe (`down` -> `probing`), and success returns it to `up`. An auth failure never auto-recovers.
5. If persistent, switch primary provider in config or rotate credentials

### BackupMissing

**What it means:** Backups are enabled but no valid backup set exists on disk — `aletheia_backup_last_success_unixtime_seconds` is `0`.

**Impact:** No recovery point exists at all. This is the first-boot and never-succeeded case, and it is more severe than a stale backup.

**Steps:**
1. Confirm the backup directory is readable and is the path configured under `[maintenance.backup]`. An unreadable directory reports `0` deliberately, so this alert also covers a permissions or mount fault.
2. Check `aletheia_backup_duration_seconds{status="error"}` for failed runs (failures are always recorded).
3. Look for a stranded staging directory in the backup dir: a set only becomes valid once it is renamed into place with a parseable `manifest.json`.
4. Run a manual backup and confirm the gauge becomes non-zero.

### BackupStale

**What it means:** The newest backup on disk is older than twice the configured backup interval (`aletheia_backup_interval_seconds`), not a fixed 48 hours.

**Impact:** Data loss risk. The instance cannot be restored to a recent point in time.

**Steps:**
1. Check `aletheia_backup_duration_seconds{status="error"}` for failed backup runs (failures are always recorded).
2. If there are no recent `status="ok"` samples, recent runs either failed or were skipped (skipped runs record nothing). Check the daemon's whole-instance backup task.
3. Check cron timer: `systemctl --user list-timers | grep aletheia`
4. Review backup script logs: `journalctl --user -u aletheia-health --since "48 hours ago"`
5. Test a restore from the latest backup set

### BackgroundTaskFailures

**What it means:** Daemon background task failures occurred in the last 5 minutes.

**Impact:** Silent data loss. Distillation, extraction, or garbage collection may skip cycles.

**Steps:**
1. Identify the failing task from the `task_type` label
2. Check logs for the specific failure: `journalctl --user -u aletheia --since "10 minutes ago" | grep <task_type>`
3. For `self_prompt` failures, verify the target agent is healthy
4. For `gc` or `drift-detection` failures, check disk space and store permissions
5. Retry manually if applicable: `aletheia maintenance run <task_name> --verbose`

### WatchdogHungProcesses

**What it means:** One or more processes registered with the watchdog have missed their heartbeat deadline.

**Impact:** Subsystem may be stuck. Watchdog will attempt restart. If max restarts exceeded, the process is abandoned.

**Steps:**
1. List hung processes from the gauge value
2. Check logs for heartbeat misses: `journalctl --user -u aletheia --since "10 minutes ago" | grep "hung process"`
3. If the process is an agent (nous actor), check its session load: `aletheia status`
4. If watchdog restarts are failing, review `aletheia_watchdog_restarts_total`
5. Restart the whole service if processes enter `Abandoned` state

### StreamEventsDropped

**What it means:** Streaming events were dropped because the channel was full or the receiver disconnected.

**Impact:** Clients miss tokens or stream termination. SSE connections may appear to hang.

**Steps:**
1. Check the `reason` label (`full` or `disconnected`)
2. If `full`, the consumer is slower than the producer. Check client read speed or network latency
3. If `disconnected`, clients are dropping connections mid-stream. Check load balancer idle timeouts
4. Review `aletheia_active_sessions` for a sudden spike in concurrent streams

### NousInboxSaturation

**What it means:** A send to a bounded nous actor inbox waited until the configured send timeout and returned `service_busy` to the caller. Alert on sustained saturation, for example `increase(aletheia_nous_inbox_saturation_total{reason="send_timeout"}[5m]) > 0` for user-facing agents.

**Impact:** New turns cannot reach the actor promptly. Clients may see service-busy errors while existing turns, tool calls, or sub-agent work continue to occupy the actor.

**Steps:**
1. Identify the saturated actor from the `nous_id` label.
2. Check whether a long turn or tool call is blocking progress: `journalctl --user -u aletheia --since "10 minutes ago" | grep <nous_id>`.
3. Compare with provider latency and in-flight request metrics; slow LLM calls can keep actor work queued.
4. Reduce concurrent client sends or sub-agent fan-out for that actor.
5. If saturation persists after traffic drops, restart the service and inspect actor panic/degraded-state logs.

### ExtractionConfidenceInflation

**What it means:** More than 5% of extraction batches over 10 minutes had >80% of facts with confidence >= 0.95.

**Impact:** The extractor is assigning unrealistically high confidence, which inflates admission and contradiction rates and degrades memory quality.

**Steps:**
1. Slice by `provider` and `model` to identify the offending producer.
2. Review the extraction prompt for the affected turn types; consider tightening the confidence calibration instructions.
3. Check whether the LLM temperature or model version changed recently.
4. If the rate is sustained, temporarily raise the refinement confidence threshold or switch model.

### ExtractionHighRejectionRate

**What it means:** More than 50% of extracted facts were rejected during refinement over a 10-minute window.

**Impact:** The extractor is emitting low-value or malformed facts (empty fields, self-references, trivial content, low confidence). Memory growth stalls while token cost stays high.

**Steps:**
1. Break down `aletheia_extraction_quality_total{status="rejected"}` by `reason`.
2. If `empty_field` or `self_reference` dominates, improve entity normalization or prompt instructions.
3. If `low_confidence` dominates, check whether the model is hedging on the input or the confidence threshold is miscalibrated.
4. If `trivial` dominates, tune the prompt to skip metadata-heavy turns.

### ExtractionContradictionSpike

**What it means:** Contradictions detected during extraction exceeded 1% of admitted facts over 10 minutes.

**Impact:** The knowledge store is accumulating mutually inconsistent facts, reducing recall precision and trust.

**Steps:**
1. Slice by `nous_id` and `producer` to find the affected cohort.
2. Inspect examples of conflicting facts via the recall API or conflict reports.
3. If contradictions follow a model change, the new extractor may disagree with older facts; consider marking older facts as stale or re-extracting.
4. If a single agent produces many contradictions, review its context window or extraction cadence.

### ExtractionLowConfidenceAdmissionSpike

**What it means:** More than 10% of admitted facts had source confidence below 0.5 over 10 minutes.

**Impact:** The admission gate is letting weakly supported claims into long-term memory, raising hallucination and contradiction risk.

**Steps:**
1. Compare `aletheia_knowledge_low_confidence_admissions_total` against `aletheia_knowledge_admission_total{outcome="admitted"}` by `nous_id`.
2. If admissions spike after a model switch, review the new model's calibration.
3. If a specific fact type dominates, consider raising its admission threshold or adding a per-type prior.
4. For transient spikes, verify that the low-confidence facts are not corrections that should supersede older facts.

### MemoryHealthScoreLow

**What it means:** The composite memory health score (`aletheia_memory_health_score`) stayed below 0.4 for 30 minutes -- the same red-band threshold `theatron/proskenion`'s Memory Health panel uses for its own client-side score.

**Impact:** Low confidence, a high orphan ratio, and/or heavy staleness are compounding; recall quality and trust in stored facts are degraded.

**Steps:**
1. Check `aletheia_memory_avg_confidence`, `aletheia_memory_orphan_ratio`, and `aletheia_memory_staleness_ratio` individually to identify which component is driving the score down.
2. Cross-check against `GET /api/v1/knowledge/check` (`GraphCheckReport`) for orphaned-entity and dangling-edge counts.
3. If staleness dominates, review whether a distillation/decay pass is running on schedule.
4. If confidence dominates, check for a recent extraction-quality regression (see the `Extraction*` alerts above).
5. Open the TUI's Memory Health panel to see the same score computed client-side; persistent divergence between the two (beyond normal query-timing skew) suggests a visibility-scope or query bug worth filing, not just a data-quality issue.

### MemoryOrphanRatioHigh

**What it means:** Over 20% of entities have no relationship and no fact link, sustained for 1 hour.

**Impact:** A large fraction of the entity graph is disconnected from anything queryable via traversal or fact lookup -- effectively dead weight in recall and graph views.

**Steps:**
1. Run `GET /api/v1/knowledge/check` for the current `orphaned_entity_count` and `entity_count`.
2. Sample a few orphaned entities (via the knowledge browser) to determine whether they are stale test/import artifacts or genuinely unlinked real entities.
3. If a bulk import introduced the orphans, consider a cleanup pass or backlinking.
4. If the ratio was already elevated before this deployment, treat the threshold as informational and tune it per-instance rather than paging on it.

### MemoryStalenessRatioHigh

**What it means:** Over 15% of active (non-forgotten, non-superseded) facts have gone unreviewed past the 30-day staleness threshold, sustained for 1 hour.

**Impact:** A growing share of memory reflects state that may no longer be current, raising the risk of recall surfacing outdated information as if it were fresh.

**Steps:**
1. Check whether a scheduled distillation/decay/review pass is configured and running.
2. Sample stale facts (sorted oldest-`recorded_at`-first) to judge whether they are genuinely outdated or simply low-churn-but-still-valid.
3. If staleness is expected for this deployment's usage pattern (e.g. a slow-moving knowledge domain), raise the alert threshold rather than treating every breach as an incident.

---

## Tuning guidance

### Thresholds

Default thresholds target a single-node deployment with moderate traffic. Adjust these for your environment:

| Factor | Increase threshold when | Decrease threshold when |
|--------|------------------------|------------------------|
| HTTP 5xx rate | Large user base with occasional provider blips | Small team where any 5xx is abnormal |
| LLM p95 latency | Using slower models (Opus, o1) | Using fast models (Haiku, GPT-4o-mini) |
| Backup staleness | Daily backups are acceptable | Compliance requires hourly backups |
| Background task failures | High agent count creates noise | Low traffic makes any failure significant |

### Label cardinality

HTTP paths are normalized (IDs replaced with `{id}`) to prevent label explosion. Do not disable normalization. If you add custom middleware that records new labels, keep cardinality under 100 unique combinations per metric.

### Scraping

Scrape the `/metrics` endpoint every 15 seconds. The endpoint is cheap but not free. Do not scrape more frequently than 5 seconds.

### Retention

Prometheus retention for Aletheia metrics should cover at least 30 days. SLO calculations and backup-staleness alerts need historical counters.
