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
| `aletheia_llm_circuit_breaker_transitions_total` | Counter | `provider`, `from`, `to` | Circuit breaker state changes |
| `aletheia_llm_concurrency_limit` | Gauge | `provider` | Current adaptive concurrency limit |
| `aletheia_llm_concurrency_in_flight` | Gauge | `provider` | In-flight requests |
| `aletheia_llm_concurrency_latency_ewma_seconds` | Gauge | `provider` | EWMA latency estimate used by the limiter |

### Agent pipeline

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_pipeline_turns_total` | Counter | `nous_id` | Turns processed per agent |
| `aletheia_pipeline_stage_duration_seconds` | Histogram | `nous_id`, `stage` | Per-stage latency (`context`, `recall`, `execute`, etc.) |
| `aletheia_pipeline_errors_total` | Counter | `nous_id`, `stage`, `error_type` | Errors by pipeline stage |
| `aletheia_tool_failures_total` | Counter | `nous_id`, `tool_name` | Tool execution failures |
| `aletheia_stream_events_dropped_total` | Counter | `nous_id`, `reason` | Streaming events dropped (`full` or `disconnected`) |
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
| `aletheia_sessions_total` | Counter | `nous_id`, `session_type` | Sessions created |
| `aletheia_backup_duration_seconds` | Histogram | `status` | Backup duration (`ok` or `error`) |

> **Note:** `aletheia_backup_duration_seconds` is registered but the production call site was removed in #3446. It is retained for future fjall backup integration. Backup staleness alerting uses this metric and will activate only after backup recording is reattached.

### Knowledge and embeddings

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `aletheia_knowledge_facts_total` | Counter | `nous_id` | Facts inserted |
| `aletheia_knowledge_extractions_total` | Counter | `nous_id`, `status` | Extraction operations |
| `aletheia_recall_duration_seconds` | Histogram | `nous_id` | Recall query latency |
| `aletheia_embedding_duration_seconds` | Histogram | `provider` | Embedding computation latency |

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
| Backup freshness | Deployment-defined | Backup metric is registered but inactive until backup recording is reattached |
| Hung processes | 0 | `aletheia_watchdog_hung_processes` |

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

**What it means:** A circuit breaker transitioned to `open` state within the last 5 minutes.

**Impact:** Requests to that provider are failing fast. Fallback or retry logic is active.

**Steps:**
1. Identify the provider from the `provider` label
2. Check provider health and credentials
3. Review `aletheia_llm_requests_total{status="error"}` for error patterns
4. If transient, the circuit should auto-recover to `half_open` then `closed`
5. If persistent, switch primary provider in config or rotate credentials

### BackupStale

**What it means:** No successful backup has completed in the last 48 hours.

**Impact:** Data loss risk. Session store cannot be restored to a recent point in time.

**Steps:**
1. Confirm whether your deployment has wired backup recording to `aletheia_backup_duration_seconds`.
2. If the metric is empty, backup recording is not yet wired to fjall. Run a manual backup using your deployment's backup process.
3. Check cron timer: `systemctl --user list-timers | grep aletheia`
4. Review backup script logs: `journalctl --user -u aletheia-health --since "48 hours ago"`
5. Test a restore from the latest backup file

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
