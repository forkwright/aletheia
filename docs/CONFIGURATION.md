# Configuration Reference

**File:** `instance/config/aletheia.toml`

Loaded by the `taxis` crate using figment with a three-layer cascade:

1. Compiled defaults (`AletheiaConfig::default()`)
2. TOML file (if present)
3. Environment variables, prefix `ALETHEIA_` (double underscore for nesting: `ALETHEIA_GATEWAY__PORT=9000`)

Later layers override earlier ones. All field names use `snake_case` in YAML; `camelCase` also works via serde compat.

---

## Table of Contents

- [agents](#agents)
- [gateway](#gateway)
- [channels](#channels)
- [bindings](#bindings)
- [embedding](#embedding)
- [data](#data)
- [maintenance](#maintenance)
- [pricing](#pricing)
- [packs](#packs)

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
| `max_tool_iterations` | u32 | `50` | Safety limit on consecutive tool use per turn |
| `allowed_roots` | string[] | `[]` | Filesystem paths the agent may access |
| `tool_timeouts` | object | see below | Per-tool execution timeout overrides |

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

```yaml
agents:
  defaults:
    model:
      primary: claude-sonnet-4-6
    context_tokens: 200000
    thinking_enabled: false
    tool_timeouts:
      default_ms: 120000
      overrides:
        exec: 300000

  list:
    - id: main
      default: true
      workspace: /srv/aletheia/instance/nous/main

    - id: research
      name: Scholar
      workspace: /srv/aletheia/instance/nous/research
      model:
        primary: claude-opus-4-6
        fallbacks:
          - claude-sonnet-4-6
      thinking_enabled: true
      domains:
        - research
        - analysis
```

---

## gateway

HTTP gateway serving the API.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | u16 | `18789` | Listen port |
| `bind` | string | `"localhost"` | Bind mode: `"localhost"` (loopback only), `"lan"` (all interfaces), or a custom address |
| `auth.mode` | string | `"token"` | Auth mode: `"token"` (bearer) or `"none"` |

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
| `requestsPerMinute` | u32 | `60` | Maximum requests per minute per client IP |

```yaml
gateway:
  port: 18789
  bind: localhost
  auth:
    mode: token
  tls:
    enabled: true
    cert_path: config/tls/cert.pem
    key_path: config/tls/key.pem
  cors:
    allowed_origins:
      - "https://my-dashboard.local"
  body_limit:
    max_bytes: 2097152
  csrf:
    enabled: true
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
| `dm_policy` | string | `"contacts"` | DM access: `"contacts"` (known contacts only), `"open"` (anyone), `"allowlist"` |
| `group_policy` | string | `"allowlist"` | Group access: `"open"`, `"allowlist"` |
| `require_mention` | bool | `true` | Require @mention in groups |
| `send_read_receipts` | bool | `true` | Send read receipts |
| `text_chunk_limit` | u32 | `2000` | Max chars per outgoing message chunk |

```yaml
channels:
  signal:
    enabled: true
    accounts:
      default:
        account: "+15551234567"
        http_host: localhost
        http_port: 8080
        dm_policy: contacts
        group_policy: allowlist
        require_mention: true
```

---

## bindings

Array of routing rules mapping channel sources to agents. Evaluated in order — first match wins.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `channel` | string | yes | -- | Channel type (e.g. `"signal"`) |
| `source` | string | yes | -- | Source pattern: phone number, group ID, or `"*"` |
| `nous_id` | string | yes | -- | Agent to route to |
| `session_key` | string | no | `"{source}"` | Session key pattern. Supports `{source}` and `{group}`. |

```yaml
bindings:
  - channel: signal
    source: "+15559876543"
    nous_id: research

  - channel: signal
    source: "*"
    nous_id: main
```

More specific bindings should appear first.

---

## embedding

Embedding provider for the recall pipeline (vector search over knowledge).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `provider` | string | `"mock"` | Provider type: `"mock"`, `"candle"` |
| `model` | string | -- | Provider-specific model name |
| `dimension` | usize | `384` | Output vector dimension (must match HNSW index) |

```yaml
embedding:
  provider: candle
  model: BAAI/bge-small-en-v1.5
  dimension: 384
```

The `mock` provider returns zero vectors — useful for development without loading ML models.

---

## data

### data.retention

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `session_max_age_days` | u32 | `90` | Max age for closed sessions |
| `orphan_message_max_age_days` | u32 | `30` | Max age for orphaned messages |
| `max_sessions_per_nous` | u32 | `0` | Max sessions per agent (0 = unlimited) |
| `archive_before_delete` | bool | `true` | Export sessions to JSON before deletion |

```yaml
data:
  retention:
    session_max_age_days: 90
    archive_before_delete: true
```

---

## maintenance

Background maintenance tasks. All run automatically when the server is running, and can be triggered via `aletheia maintenance run <task>`.

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

```yaml
maintenance:
  trace_rotation:
    enabled: true
    max_age_days: 7
    compress: true
  drift_detection:
    enabled: true
  db_monitoring:
    warn_threshold_mb: 200
    alert_threshold_mb: 1000
  retention:
    enabled: true
```

---

## pricing

Per-model pricing for cost estimation in Prometheus metrics. Keyed by model name.

| Field | Type | Description |
|-------|------|-------------|
| `input_cost_per_mtok` | f64 | Cost per million input tokens (USD) |
| `output_cost_per_mtok` | f64 | Cost per million output tokens (USD) |

```yaml
pricing:
  claude-sonnet-4-6:
    input_cost_per_mtok: 3.0
    output_cost_per_mtok: 15.0
  claude-opus-4-6:
    input_cost_per_mtok: 15.0
    output_cost_per_mtok: 75.0
```

---

## packs

Array of filesystem paths to external domain packs. Each path should be a directory containing `pack.yaml`. See [PACKS.md](PACKS.md).

```yaml
packs:
  - /srv/aletheia/packs/engineering
  - /srv/aletheia/packs/research
```

---

## Environment Variables

Any config key can be set via environment variable with the `ALETHEIA_` prefix and double underscores for nesting:

| Config Key | Environment Variable |
|------------|---------------------|
| `gateway.port` | `ALETHEIA_GATEWAY__PORT` |
| `gateway.bind` | `ALETHEIA_GATEWAY__BIND` |
| `embedding.provider` | `ALETHEIA_EMBEDDING__PROVIDER` |
| `channels.signal.enabled` | `ALETHEIA_CHANNELS__SIGNAL__ENABLED` |

The `ANTHROPIC_API_KEY` environment variable is read separately by the provider registry (not part of the config cascade).

---

## Minimal Config

```yaml
agents:
  list:
    - id: main
      default: true
      workspace: /path/to/instance/nous/main
```

Everything else has sensible defaults. Add sections as needed.
