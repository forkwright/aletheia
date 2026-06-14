# Configuration reference

**File:** `instance/config/aletheia.toml`

Loaded by the `taxis` crate using an owned TOML loader with a three-layer cascade:

1. Compiled defaults (`AletheiaConfig::default()`)
2. TOML file (if present)
3. Environment variables, prefix `ALETHEIA_` (double `_` for nesting: `ALETHEIA_GATEWAY__PORT=9000`)

Later layers override earlier ones. All config keys use `camelCase` in TOML — taxis structs are annotated `#[serde(rename_all = "camelCase")]` throughout. A small set of legacy keys also accepts `snake_case` aliases, but `camelCase` is canonical and required for new keys.

---

## Cascade terminology

Aletheia uses the same override vocabulary across configuration and agent workspace files:

| Fleet tier | Aletheia config equivalent | Workspace equivalent |
|------------|----------------------------|----------------------|
| Repo default | Compiled defaults and `instance.example/` | Template files under `instance.example/nous/_template/` |
| Team or instance | `instance/config/aletheia.toml` | Shared guidance under `instance/shared/` and `instance/theke/` |
| Agent or personal | Per-agent entries in `agents.list[]` | `instance/nous/{id}/` identity, memory, goals, and tool notes |
| Environment override | `ALETHEIA_` variables | Process environment and allowed filesystem roots |

Runtime configuration uses the three-layer TOML cascade above. Agent bootstrap files use a workspace cascade through `nous/{id}/`, `shared/`, `theke/`, and configured domain packs. In both cases, the narrowest scope should only carry values that genuinely need to differ from the broader tier.

---

## Table of contents

- [agents](#agents)
- [gateway](#gateway)
- [jwt](#jwt)
- [channels](#channels)
- [bindings](#bindings)
- [embedding](#embedding)
- [credential](#credential)
- [providers](#providers)
- [provider capability matrix](#provider-capability-matrix)
- [data](#data)
- [nous_behavior](#nous_behavior)
- [daemon_behavior](#daemon_behavior)
- [tool_limits](#tool_limits)
- [maintenance](#maintenance)
- [dispatch](#dispatch)
- [logging](#logging)
- [pricing](#pricing)
- [packs](#packs)
- [sandbox](#sandbox)
- [Environment variables](#environment-variables)
- [Minimal config](#minimal-config)

---

## agents

Contains `defaults` (inherited by all agents) and `list` (per-agent definitions).

### agents.defaults

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `model.primary` | string | `"claude-sonnet-4-6"` | Primary model ID |
| `model.fallbacks` | string[] | `[]` | Fallback model IDs, tried in order |
| `context_tokens` | u32 | `200000` | Context window budget (tokens) |
| `max_output_tokens` | u32 | `16384` | Max tokens per response |
| `bootstrap_max_tokens` | u32 | `40000` | Max tokens for bootstrap context injection |
| `user_timezone` | string | `"UTC"` | IANA timezone for time-aware prompts |
| `timeout_seconds` | u32 | `300` | LLM call timeout |
| `thinking_enabled` | bool | `false` | Enable extended thinking |
| `thinking_budget` | u32 | `10000` | Max tokens for extended thinking |
| `max_tool_iterations` | u32 | `200` | Safety limit on consecutive tool use per turn |
| `allowed_roots` | string[] | `[]` | Filesystem paths the agent may access |
| `toolGroups` | `"all"`, `"deny"`, or string[] | `"deny"` | Tool-group policy. Missing or empty values deny all groups. |
| `tool_timeouts` | object | see `agents.defaults.tool_timeouts` section | Per-tool execution timeout overrides |
| `working_state_ttl_secs` | u64 | `604800` | Working-state expiry window (7 days) |
| `working_state_max_task_stack` | usize | `10` | Maximum working-state task stack depth before oldest entries are evicted |
| `tool_datalog_default_timeout_secs` | f64 | `5.0` | Default Datalog memory tool timeout |

#### agents.defaults.caching

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether prompt caching is active |
| `strategy` | string | `"auto"` | Caching strategy: `"auto"` (cache system prompt and large context blocks) or `"disabled"` |

#### agents.defaults.tool_timeouts

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_ms` | u64 | `120000` | Default timeout for all tools (ms) |
| `overrides` | map<string, u64> | `{}` | Per-tool timeout overrides keyed by tool name |

### agents.list[]

Each entry defines a nous (agent). Fields not specified inherit from `agents.defaults`.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Unique agent identifier (matches `nous/{id}/` directory) |
| `name` | string | no | -- | Display name |
| `default` | bool | no | `false` | Default agent for unrouted messages |
| `workspace` | string | yes | -- | Path to agent workspace directory |
| `model` | object | no | inherits | Per-agent model override `{ primary, fallbacks }` |
| `thinking_enabled` | bool | no | inherits | Per-agent thinking override |
| `allowed_roots` | string[] | no | `[]` | Additional filesystem roots (merged with defaults) |
| `domains` | string[] | no | `[]` | Knowledge domains (e.g. `"code"`, `"research"`) |

```toml
[agents.defaults.model]
primary = "claude-sonnet-4-6"

[agents.defaults]
context_tokens = 200000
thinking_enabled = false
toolGroups = ["read", "edit", "command", "mcp", "spawn_subtask", "plan", "verify"]

[agents.defaults.tool_timeouts]
default_ms = 120000

[agents.defaults.tool_timeouts.overrides]
exec = 300000

[[agents.list]]
id = "main"
default = true
workspace = "/srv/aletheia/instance/nous/main"

[[agents.list]]
id = "research"
name = "Scholar"
workspace = "/srv/aletheia/instance/nous/research"
thinking_enabled = true
domains = ["research", "analysis"]

[agents.list.model]
primary = "claude-opus-4-6"
fallbacks = ["claude-sonnet-4-6"]
```

`toolGroups` is fail-closed. If the field is absent, set to `"deny"`, or set
to `[]`, the agent receives no grouped tools. Use `"all"` only for an explicit
admin/full-access policy; use an array for normal role-limited access.

`nous::PipelineConfig` also carries per-stage turn budgets (`context`,
`recall`, `history`, `guard`, `execute`, `finalize`, `reflection`, and
`total`). These are runtime pipeline limits; top-level actor and manager
timeouts live under `nous_behavior`.

---

## gateway

HTTP gateway serving the API.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | u16 | `18789` | Listen port |
| `bind` | string | `"localhost"` | Bind mode: `"localhost"` (loopback only), `"lan"` (all interfaces), or a custom address |
| `auth.mode` | string | `"token"` | Auth mode: `"token"` (bearer) or `"none"` |
| `auth.none_role` | string | `"admin"` | Role assigned to anonymous requests when `auth.mode = "none"`; valid values are `"readonly"`, `"agent"`, `"operator"`, and `"admin"` |

### gateway.tls

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether TLS termination is active |
| `cert_path` | string | -- | Path to PEM-encoded certificate file |
| `key_path` | string | -- | Path to PEM-encoded private key file |

### gateway.cors

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allowed_origins` | string[] | `[]` | Allowed origins. Empty or `["*"]` is permissive. |
| `max_age_secs` | u64 | `3600` | Preflight cache duration (seconds) |

### gateway.body_limit

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_bytes` | usize | `1048576` | Maximum request body size (1 MB) |

### gateway.csrf

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether CSRF header checking is active |
| `header_name` | string | `"x-requested-with"` | Required header name |
| `header_value` | string | `"aletheia"` | Required header value |

### gateway.rate_limit

Per-IP rate limiting for API endpoints. Requests that exceed the limit receive
`429 Too Many Requests` with a `Retry-After` header indicating when to retry.

The client IP is read from `X-Forwarded-For` or `X-Real-IP` (reverse proxy)
and falls back to `127.0.0.1` for direct connections.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether rate limiting is active |
| `requests_per_minute` | u32 | `60` | Maximum requests per minute per client IP |

```toml
[gateway]
port = 18789
bind = "localhost"

[gateway.auth]
mode = "token"

[gateway.tls]
enabled = true
cert_path = "config/tls/cert.pem"
key_path = "config/tls/key.pem"

[gateway.cors]
allowed_origins = ["https://my-dashboard.local"]

[gateway.body_limit]
max_bytes = 2097152

[gateway.csrf]
enabled = true
```

---

## jwt

JWT validation tuning. Applies to every bearer token the gateway accepts.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `clock_skew_leeway_secs` | u64 | `30` | Seconds of clock drift tolerated when checking the `exp` claim. A token whose `exp` lies up to this many seconds in the past is still accepted. Set to `0` for strict expiry on tightly synchronized hosts. |

```toml
[jwt]
clock_skew_leeway_secs = 30
```

---

## channels

### channels.signal

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable Signal channel |
| `accounts` | map<string, account> | `{}` | Named Signal account configs |

### channels.signal.accounts.*

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | -- | Display label |
| `enabled` | bool | `true` | Enable this account |
| `account` | string | -- | Phone number (e.g. `"+15551234567"`) |
| `http_host` | string | `"localhost"` | signal-cli JSON-RPC host |
| `http_port` | u16 | `8080` | signal-cli JSON-RPC port |
| `cli_path` | string | -- | Path to signal-cli binary (auto-detected if unset) |
| `auto_start` | bool | `true` | Auto-start receive loop |

```toml
[channels.signal]
enabled = true

[channels.signal.accounts.default]
account = "+15551234567"
http_host = "localhost"
http_port = 8080
auto_start = true
```

The following policy fields are **not implemented** in the current runtime and
are not accepted by the strict config schema: `dm_policy`, `group_policy`,
`require_mention`, `send_read_receipts`, and `text_chunk_limit`. Inbound Signal
routing and message handling are controlled by the channel bindings (see
[bindings](#bindings)) and by signal-cli's own settings.

---

## bindings

Array of routing rules mapping channel sources to agents. The Agora router
uses a fixed specificity order; it does **not** use declaration order or first
match. The order of `[[bindings]]` entries in the config file does not affect
routing.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `channel` | string | yes | -- | Channel type (e.g. `"signal"`) |
| `source` | string | yes | -- | Source pattern: phone number, group ID, or `"*"` |
| `nous_id` | string | yes | -- | Agent to route to |
| `session_key` | string | no | `"{source}"` | Session key pattern. Supports `{source}` and `{group}`. |

```toml
[[bindings]]
channel = "signal"
source = "*"
nous_id = "main"

[[bindings]]
channel = "signal"
source = "+15559876543"
nous_id = "research"
```

### Routing precedence

The router resolves each inbound message in the following order (`crates/agora/src/router.rs`):

1. **Group binding** — exact match on `channel` + `group_id`.
2. **Source binding** — exact match on `channel` + sender/source.
3. **Channel default** — `channel` + `source = "*"`.
4. **Global default** — the agent configured with `default: true`.
5. **No match** — message is dropped.

A wildcard `source = "*"` entry before an exact source entry does **not** win;
the exact source binding always takes precedence. Use the most specific binding
you need and rely on the fixed order above.

---

## embedding

Embedding provider for the recall pipeline (vector search over knowledge).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `provider` | string | `"mock"` | Provider type: `"mock"`, `"candle"`, `"openai-compat"`, `"voyage"` |
| `model` | string | -- | Provider-specific model name |
| `dimension` | usize | `384` | Output vector dimension (must match HNSW index) |

```toml
[embedding]
provider = "candle"
model = "BAAI/bge-small-en-v1.5"
dimension = 384
```

The `mock` provider returns zero vectors, useful for development without loading ML models.

---

## credential

Controls how the server discovers LLM API credentials. The `source` field selects the resolution strategy.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source` | string | `"auto"` | Credential strategy: `"auto"` (instance file, then env vars, then Claude Code credentials), `"api-key"` (instance file and env vars only), `"claude-code"` (prefer Claude Code credentials) |
| `claude_code_credentials` | string | `null` | Override path to the Claude Code credentials file. Resolves to `~/.claude/.credentials.json` when unset. |

```toml
[credential]
source = "auto"
claude_code_credentials = "~/.claude/.credentials.json"
```

---

## providers

`[[providers]]` entries declare available LLM providers declaratively. The runtime registers them in list order; model routing picks the first provider that advertises the requested model. Provider kinds are defined in `crates/taxis/src/config/behavior/provider.rs` and registered in `crates/aletheia/src/runtime/setup.rs`.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Operator-facing label, must be unique across the list. |
| `providerType` | string | yes | `anthropic`, `openai`, `openai-compatible`, `claude-code`, or `codex-oauth`. |
| `apiFamily` | string | no | For `openai`: `responses` (default) or `chat-completions`. For `openai-compatible`: `chat-completions` (default). Ignored for other kinds. |
| `baseUrl` | string | no | HTTP base URL. Required for `openai-compatible`; optional for `anthropic` (defaults to `https://api.anthropic.com`); ignored for subprocess adapters. |
| `apiKeyEnv` | string | no | Environment variable holding the API key. Read at startup via `std::env::var`. Optional for loopback providers without auth. |
| `deploymentTarget` | string | no | `cloud` (default), `local-hosted`, or `embedded`. Drives the fact-sensitivity filter and air-gapped mode. |
| `models` | string[] | no | Model identifiers this provider advertises. The first provider in list order claiming a model wins. |

```toml
[[providers]]
name = "anthropic-cloud"
providerType = "anthropic"
apiKeyEnv = "ANTHROPIC_API_KEY"
deploymentTarget = "cloud"
models = ["claude-sonnet-4-6"]

[[providers]]
name = "openai-cloud"
providerType = "openai"
apiKeyEnv = "OPENAI_API_KEY"
apiFamily = "responses"
deploymentTarget = "cloud"
models = ["gpt-5.3-codex"]

[[providers]]
name = "local-llama"
providerType = "openai-compatible"
baseUrl = "http://127.0.0.1:8088/v1"
deploymentTarget = "embedded"
models = ["llama3.1-70b"]
```

### Provider capability matrix

| Provider path | Credential source | Simple chat + recall | Aletheia organon tool-loop | Notes |
|---|---|---|---|---|
| Native Anthropic provider | `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN` | yes | yes | Messages API. Declarative `anthropic` entries require `apiKeyEnv` to avoid double-registering the first-party provider. |
| OpenAI cloud (`openai`) | `OPENAI_API_KEY` | yes | yes | First-party `/v1/responses` by default; set `apiFamily = "chat-completions"` for the legacy endpoint. |
| OpenAI-compatible local/third-party (`openai-compatible`) | Optional (`apiKeyEnv`) | yes | yes | `/v1/chat/completions` wire format for llama.cpp, ollama, vllm, and compatible proxies. |
| Claude Code subprocess (`claude-code`) | Local Claude Code OAuth seat | yes | no | Feature-gated (`cc-provider`); registered via the credential chain, declarative entries are accepted but skipped by the registry to avoid duplicates. |
| Codex subprocess (`codex-oauth`) | Local Codex seat | yes | no | Feature-gated (`codex-provider`); registered via the credential chain, declarative entries are accepted but do not change startup behavior. |

The `aletheia add-nous` scaffolding command currently validates only `anthropic` and `openai` provider strings and checks for `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`. Other provider kinds must be configured manually in `aletheia.toml`.

---

## data

### data.retention

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `session_max_age_days` | u32 | `90` | Max age for closed sessions |
| `orphan_message_max_age_days` | u32 | `30` | Max age for orphaned messages |
| `max_sessions_per_nous` | u32 | `0` | Max sessions per agent (0 = unlimited) |
| `archive_before_delete` | bool | `true` | Export sessions to JSON before deletion |

```toml
[data.retention]
session_max_age_days = 90
archive_before_delete = true
```

---

## nous_behavior

Actor and manager behavior thresholds. These fields are hot-reloadable in the
config registry unless the runtime code documents a colder lifecycle.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `degraded_panic_threshold` | u32 | `5` | Panics within the window before degraded mode |
| `degraded_window_secs` | u64 | `600` | Panic counting window |
| `inbox_recv_timeout_secs` | u64 | `30` | Actor inbox receive timeout before warning |
| `manager_ping_timeout_secs` | u64 | `5` | Health ping timeout |
| `stuck_turn_timeout_secs` | u64 | `600` | Active turn duration before the manager treats the actor as stuck |
| `loop_detection_window` | usize | `50` | Recent tool-call window scanned for loop patterns |
| `cycle_detection_max_len` | usize | `10` | Maximum repeated sequence length examined |
| `bootstrap_cache_ttl_secs` | u64 | `60` | Bootstrap file cache TTL; `0` disables the cache |
| `shutdown_timeout_secs` | u64 | `30` | Graceful shutdown bound before actor tasks are aborted |

```toml
[nous_behavior]
loop_detection_window = 50
stuck_turn_timeout_secs = 600
shutdown_timeout_secs = 30
```

---

## daemon_behavior

Daemon watchdog, prosoche anomaly detection, and runner output summarization.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `watchdog_backoff_base_secs` | u64 | `2` | Base watchdog restart backoff |
| `watchdog_backoff_cap_secs` | u64 | `300` | Maximum watchdog restart backoff |
| `prosoche_anomaly_sample_size` | usize | `15` | Samples used for prosoche anomaly detection |
| `runner_output_brief_head_lines` | usize | `5` | Head lines kept in task output summaries |
| `runner_output_brief_tail_lines` | usize | `3` | Tail lines kept in task output summaries |

---

## tool_limits

Deployment-wide organon tool size and timeout limits. Agent-specific overrides
still belong under `agents.defaults.tool_timeouts`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `glob_timeout_secs` | u64 | `10` | Filesystem glob timeout |
| `subprocess_timeout_secs` | u64 | `60` | Default subprocess timeout |
| `inter_session_max_timeout_secs` | u64 | `300` | Maximum wait for inter-session messages |
| `agent_dispatch_timeout_secs` | u64 | `300` | Default timeout for spawned sub-agent dispatch |
| `datalog_default_timeout_secs` | f64 | `5.0` | Default timeout for the Datalog memory tool |

---

## maintenance

Background maintenance tasks. Some run automatically when the server is running; others are opt-in or not yet implemented. Tasks can also be triggered manually via `aletheia maintenance run <task>`.

### Always-on maintenance (enabled by default)

| Task | Config section | Default | Schedule |
|------|----------------|---------|----------|
| Trace rotation | `maintenance.trace_rotation` | `enabled = true` | Cron |
| Drift detection | `maintenance.drift_detection` | `enabled = true` | Cron |
| DB size monitoring | `maintenance.db_monitoring` | `enabled = true` | Cron |

### Opt-in maintenance (disabled by default)

| Task | Config section | Default | Schedule |
|------|----------------|---------|----------|
| Retention enforcement | `maintenance.retention` | `enabled = false` | Cron |
| Knowledge maintenance | `maintenance.knowledge_maintenance_enabled` | `false` | see below |
| Serendipity discovery | `maintenance.knowledge_maintenance_serendipity` | `enabled = false` | Cron |

### maintenance.trace_rotation

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether automatic trace rotation runs |
| `max_age_days` | u32 | `14` | Delete trace files older than this |
| `max_total_size_mb` | u64 | `500` | Max total trace directory size (MB) |
| `compress` | bool | `true` | Gzip-compress rotated files |
| `max_archives` | usize | `30` | Max compressed archives to retain |

### maintenance.drift_detection

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether drift detection runs |
| `alert_on_missing` | bool | `true` | Warn on files missing from expected layout |
| `ignore_patterns` | string[] | `["data/", "signal/", "*.db", ".gitkeep"]` | Glob patterns to ignore |

### maintenance.db_monitoring

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether database size monitoring runs |
| `warn_threshold_mb` | u64 | `100` | Warning threshold (MB) |
| `alert_threshold_mb` | u64 | `500` | Alert threshold (MB) |

### maintenance.retention

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether automatic retention enforcement runs |

### maintenance.knowledge_maintenance_enabled

`knowledge_maintenance_enabled` is a top-level boolean switch (not a table) that
gates all scheduled knowledge-maintenance tasks. It defaults to `false`.

When set to `true` **and** a knowledge executor is available, the daemon
registers the following implemented tasks (`crates/daemon/src/runner/registration.rs`):

| Task ID | Cadence | Purpose | Manual run |
|---------|---------|---------|------------|
| `decay-refresh` | every 4 hours | Refresh temporal decay scores | yes |
| `entity-dedup` | every 6 hours | Merge duplicate knowledge graph entities | yes |
| `graph-recompute` | every 8 hours | Recompute PageRank / centrality scores | yes |
| `skill-decay` | daily at 06:00 | Retire stale skills | yes |
| `derived-facts-materialize` | every 6 hours | Materialize derived Datalog rules | yes |

If a task completes with non-fatal errors (for example, a per-fact persistence
failure during decay refresh), the runner records the task as **degraded**,
preserves the non-fatal error count in task status/metrics, and does not treat
it as a hard failure. The outcome is surfaced as `success = false` with an
explanatory message per the existing binary task-outcome policy.

The following tasks are **not implemented or scheduled** today; `aletheia maintenance run <id>`
returns a structured "not scheduled" result and `aletheia maintenance status`
shows them as `planned`/`unavailable` (`crates/aletheia/src/commands/maintenance.rs`):

- `embedding-refresh` — requires an `EmbeddingProvider` bridge.
- `knowledge-gc` / edge pruning — no concrete store contract.
- `index-maintenance` — no concrete store contract.
- `graph-health-check` — no concrete diagnostic contract.

Implemented knowledge-maintenance tasks also return `unavailable` when the
knowledge store cannot be opened (for example, when the `recall` feature is
disabled or the knowledge database directory does not exist). `aletheia maintenance run all`
skips unavailable knowledge tasks rather than aborting the whole batch.

### maintenance.knowledge_maintenance_serendipity

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Whether serendipity discovery maintenance runs |
| `cadence` | string | `"0 0 7 * * *"` | Cron expression for the task (daily at 07:00 UTC) |

```toml
[maintenance]
knowledge_maintenance_enabled = true

[maintenance.trace_rotation]
enabled = true
max_age_days = 7
compress = true

[maintenance.drift_detection]
enabled = true

[maintenance.db_monitoring]
warn_threshold_mb = 200
alert_threshold_mb = 1000

[maintenance.retention]
enabled = true

[maintenance.knowledge_maintenance_serendipity]
enabled = true
cadence = "0 0 7 * * *"
```

---

## dispatch

Recurring energeia dispatches driven by cron expressions. Each task is parsed
on startup and scheduled by the daemon: at every scheduled tick the executor
loads the project's prompt queue, filters by `promptNumbers`, and invokes the
energeia orchestrator. Requires the `energeia` build feature.

### dispatch.cronTasks[]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | _required_ | Unique task identifier; used as the fjall lock key for cross-restart dedup. |
| `schedule` | string | _required_ | 6-field cron expression (`sec min hour dom mon dow`) parsed by `jiff-cron`. |
| `jitterSecs` | u64 | `0` | Maximum random offset (±, in seconds) applied to each computed fire time to spread thundering-herd starts. |
| `enabled` | bool | `true` | Set `false` to leave the task in the config without scheduling it. |
| `dispatchSpec` | table | _required_ | Spec passed to the orchestrator (see below). |

### dispatch.cronTasks[].dispatchSpec

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `project` | string | _required_ | Project key — resolves to `theke/projects/<project>/prompts/queue/`. |
| `promptNumbers` | u32[] | _required_ | Prompt numbers to dispatch; empty selects every prompt in the queue. |
| `dagRef` | string | `null` | Optional DAG reference handed to the orchestrator. |
| `maxParallel` | u32 | `null` | Override the orchestrator's max-concurrent-sessions budget. |
| `maxTurns` | u32 | `null` | Override the orchestrator's per-session turn budget. |

**Missed-tick policy.** The scheduler computes the next future occurrence
after `now` on each loop iteration and sleeps until then; if the daemon was
offline for several scheduled windows, only the next future tick fires (no
catch-up storm). The fjall-backed lock store also prevents the same scheduled
time from firing twice across restarts.

**Overlap policy.** When a task's previous callback is still running at the
next scheduled tick, the new fire is **skipped** with `cron.task.skipped`
emitted at warn. This prevents two concurrent dispatches from competing for
the same project worktree.

```toml
[[dispatch.cronTasks]]
name = "nightly-aletheia-sweep"
schedule = "0 0 2 * * *"   # every day at 02:00:00 UTC
jitterSecs = 60            # ± up to one minute
enabled = true

[dispatch.cronTasks.dispatchSpec]
project = "aletheia"
promptNumbers = [1, 2, 3]
maxParallel = 2
```

---

## logging

Write log files to a configurable directory with automatic retention. Set the `RUST_LOG` environment variable to control console output separately.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `log_dir` | string | `null` | Directory for daily log files. Relative paths resolve from the instance root. Defaults to `{instance}/logs/` when unset. |
| `retention_days` | u32 | `14` | Days to retain log files before deletion. Cleanup runs at startup and every 24 hours. |
| `level` | string | `"warn"` | Minimum level written to log files. Accepts any `tracing` filter directive (e.g. `"warn"`, `"error"`, `"aletheia=debug,warn"`). |

```toml
[logging]
log_dir = "/var/log/aletheia"
retention_days = 30
level = "aletheia=debug,warn"
```

---

## pricing

Per-model pricing for cost estimation in Prometheus metrics. Keyed by model name.

| Field | Type | Description |
|-------|------|-------------|
| `input_cost_per_mtok` | f64 | Cost per million input tokens (USD) |
| `output_cost_per_mtok` | f64 | Cost per million output tokens (USD) |

```toml
[pricing.claude-sonnet-4-6]
input_cost_per_mtok = 3.0
output_cost_per_mtok = 15.0

[pricing.claude-opus-4-6]
input_cost_per_mtok = 15.0
output_cost_per_mtok = 75.0
```

---

## packs

Array of filesystem paths to external domain packs. Each path should be a directory containing `pack.toml`. Relative paths resolve from the instance root (`$ALETHEIA_ROOT` or `./instance`); absolute paths are used as-is. See [PACKS.md](PACKS.md).

```toml
packs = [
    "/srv/aletheia/packs/engineering",
    "/srv/aletheia/packs/research",
]
```

---

## sandbox

Filesystem sandbox applied to tool execution. When enabled, tools are restricted to the paths explicitly listed in `agents.*.allowed_roots` plus any extra paths declared here.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Whether sandbox restrictions are applied |
| `enforcement` | string | `"permissive"` | `"enforcing"` blocks violations; `"permissive"` logs them without blocking |
| `extra_read_paths` | string[] | `[]` | Additional filesystem paths granted read access to all tools |
| `extra_write_paths` | string[] | `[]` | Additional filesystem paths granted read+write access to all tools |
| `extra_exec_paths` | string[] | `[]` | Additional filesystem paths granted execute access. Values may begin with `~`, which expands to `$HOME` at policy-build time. |
| `egress` | string | `"allow"` | Child-process network policy: `"allow"` permits outbound network, `"deny"` blocks it, and `"allowlist"` permits only listed destinations |
| `egress_allowlist` | string[] | `[]` | Addresses or CIDR ranges permitted when `egress = "allowlist"`; loopback entries are enforceable without root privileges |
| `nproc_limit` | u32 | `256` | Maximum process count (`RLIMIT_NPROC`) applied to exec child processes |

Defaults are defined in `crates/taxis/src/config/maintenance.rs` and mirrored by the execution policy in `crates/organon/src/sandbox/config.rs`; `gateway.auth.none_role` is defined in `crates/taxis/src/config/gateway.rs`.

Combined default posture: a fresh config binds the gateway to localhost and uses bearer-token auth, but rate limiting is disabled, sandbox violations are logged rather than blocked, exec child processes keep outbound network egress, and switching `gateway.auth.mode` to `"none"` without changing `gateway.auth.none_role` grants anonymous callers the `admin` role. For production-like deployments, set the restrictive values explicitly.

```toml
[sandbox]
enabled = true
enforcement = "permissive"
extra_read_paths = ["/usr/share/doc"]
extra_write_paths = []
extra_exec_paths = ["~/.cargo/bin"]
egress = "allow"
egress_allowlist = []
nproc_limit = 256
```

---

## Environment variables

The public runtime environment contract has one env-file owner:
`<instance-root>/config/env`. The root `.env.example` is the template for that
file, and `instance.example/services/aletheia.service` loads it from
`%h/aletheia/instance/config/env`.

| Variable | Owner | Meaning |
|----------|-------|---------|
| `ALETHEIA_ROOT` | `taxis::Oikos` instance discovery | Instance root only. Resolution precedence: `-r`/`--instance-root` CLI flag > `ALETHEIA_ROOT` env var > `~/aletheia/instance` default. Never a source tree or install prefix. Helper scripts follow the same precedence. |
| `ALETHEIA_BIN` | shared and deploy helper scripts | Executable path for helpers such as `shared/bin/start.sh`, `scripts/aletheia-heartbeat.sh`, deploy, rollback, and smoke scripts. Older scripts still accept `ALETHEIA_BINARY` as a compatibility fallback. |
| `ALETHEIA_ENV_FILE` | `shared/bin/start.sh` | Env file sourced at startup. Defaults to `$ALETHEIA_ROOT/config/env`, the canonical env-file owner. |
| `ALETHEIA_NOUS` | shared tools (`shared/bin/scholar`) | Nous workspace directory. Defaults to `$ALETHEIA_ROOT/nous`. |
| `ALETHEIA_CREDS` | `shared/bin/start.sh`, `credential-refresh`, `scripts/health-monitor.sh` | Anthropic credential JSON path. Defaults to `$ALETHEIA_ROOT/config/credentials/anthropic.json`. |
| `ALETHEIA_MEMORY_USER` | `shared/bin/start.sh` | Identity attributed to stored memory. Defaults to the current `whoami`. |
| `ALETHEIA_SHARED` | instance nous templates | Shared-resources root referenced by agent templates (`$ALETHEIA_SHARED/config/...`). |
| `ALETHEIA_THEKE` | instance nous templates | Vault (theke) root referenced by agent templates (`$ALETHEIA_THEKE/<domain>`). |
| `ALETHEIA_LOG_DIR` | `scripts/health-monitor.sh` | Log directory. Defaults to `$XDG_STATE_HOME/aletheia` (`~/.local/state/aletheia`). |
| `ALETHEIA_URL` | `scripts/aletheia-heartbeat.sh`, `aletheia-health.service` | Base server URL for health and heartbeat probes. Defaults to `http://127.0.0.1:18789`. |
| `ALETHEIA_HEALTH_URL` | `deploy.sh`, `rollback.sh`, `health-monitor.sh` | Health endpoint. Defaults to `http://localhost:18789/api/health`. |
| `ALETHEIA_METRICS_URL` | `scripts/health-monitor.sh` | Metrics endpoint. Defaults to `http://localhost:18789/metrics`. |
| `ALETHEIA_NOTIFY_TO` | `scripts/health-monitor.sh` | Optional `signal-cli` recipient for health alerts. |
| `ALETHEIA_HEARTBEAT_TASK` | `scripts/aletheia-heartbeat.sh` | Task id pinged by the heartbeat. Defaults to `prosoche-self-audit`. |
| `ALETHEIA_PRIMARY_KEY` | `taxis::encrypt` | Master encryption key. Overrides the instance keyfile when set. Security-sensitive. |
| `ALETHEIA_JWT_SECRET` | `taxis` gateway | JWT signing key used when `gateway.jwt_secret` is unset. Security-sensitive. |
| `ALETHEIA_ALLOW_AUTH_NONE` | `taxis::validate` | Operator gate: set to `1` to permit `auth = "none"`. Off by default. Security-sensitive. |
| `ALETHEIA_ALLOW_AUTH_NONE_LAN` | `taxis::validate` | Operator gate: set to `1` to permit `auth = "none"` on LAN binds. Off by default. Security-sensitive. |
| `SEMANTIC_SCHOLAR_API_KEY` | `shared/bin/scholar` | Optional Semantic Scholar API key for higher rate limits. |
| `ANTHROPIC_API_KEY` | credential provider chain | Anthropic API key. May live in `config/env` or the process environment. |
| `ANTHROPIC_AUTH_TOKEN` | credential provider chain | Anthropic OAuth token, usually maintained by credential tooling. |
| `VOYAGE_API_KEY` | embedding provider | Optional remote embedding provider credential. Local candle embeddings do not need it. |
| `BRAVE_SEARCH_API_KEY` | shared research tools | Optional Brave Search credential for `web_search` and operator-installed tools. |
| `PERPLEXITY_API_KEY` | shared research tools | Optional Perplexity credential for `shared/bin/pplx`. |
| `RESEARCH_EMAIL` | shared research tools | Contact email used in scholarly API user agents. |
| `PROSOCHE_GATEWAY_TOKEN` | prosoche integration | Optional token for prosoche gateway calls. |
| `PROSOCHE_CALENDAR_*` | prosoche integration | Optional calendar identifiers for prosoche calendar surfaces. |
| `CHROMIUM_PATH` | browser automation and Chromium printer | Optional explicit Chromium executable path. |
| `RUST_LOG` | logging runtime | Console log filter. |
| `RUST_BACKTRACE` | Rust runtime | Backtrace control for panics and error reports. |

Any config key can also be set via environment variable with the `ALETHEIA_`
prefix and double underscores for nesting:

| Config Key | Environment Variable |
|------------|---------------------|
| `gateway.port` | `ALETHEIA_GATEWAY__PORT` |
| `gateway.bind` | `ALETHEIA_GATEWAY__BIND` |
| `embedding.provider` | `ALETHEIA_EMBEDDING__PROVIDER` |
| `channels.signal.enabled` | `ALETHEIA_CHANNELS__SIGNAL__ENABLED` |

Provider credentials such as `ANTHROPIC_API_KEY` are read by the credential
provider chain, not by the TOML config cascade.

### Internal and test-fixture variables

These are read only by maintainer and CI tooling, not by the public runtime path:

| Variable | Owner | Meaning |
|----------|-------|---------|
| `ALETHEIA_AUTH_TOKEN` | `scripts/smoke-proskenion.sh` | Bearer token written to the temporary desktop config during the smoke check. |
| `ALETHEIA_EVAL_TOKEN` | `scripts/benchmark.sh` | Auth token used when the benchmarked instance requires authentication. |
| `ALETHEIA_SMOKE_PORT` | `scripts/smoke-proskenion.sh` | Port for the smoke check's local server. Defaults to a random port in `39000-40999`. |
| `ALETHEIA_SMOKE_KEEP_LOGS` | `scripts/smoke-proskenion.sh` | Set to `1` to retain temporary smoke logs on success. |

---

## Minimal config

```toml
[[agents.list]]
id = "main"
default = true
workspace = "/path/to/instance/nous/main"
```

Everything else has sensible defaults. Add sections as needed.
