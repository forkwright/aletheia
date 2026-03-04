# Configuration Reference

## TypeScript Runtime

**File:** `~/.aletheia/aletheia.json`

Validated at startup against the Zod schema in `src/taxis/schema.ts`. Unknown top-level fields are preserved (passthrough) for forward compatibility.

## Rust Crates

**File:** `instance/config/aletheia.yaml` (or `~/.aletheia/aletheia.yaml`)

Loaded by the `taxis` crate using figment with a three-layer cascade:

1. Compiled defaults (`AletheiaConfig::default()`)
2. YAML file (if present)
3. Environment variables, prefix `ALETHEIA_` (double underscore for nesting: `ALETHEIA_GATEWAY__PORT=9000`)

Later layers override earlier ones.

Differences from the JSON config:

- YAML format
- Figment cascade (defaults -> file -> env) vs. single-file loading
- `snake_case` is canonical; `camelCase` works via compat layer
- Secret values use `SecretString` from the `secrecy` crate - never logged or serialized

---

## Table of Contents

- [agents](#agents)
- [bindings](#bindings)
- [channels](#channels)
- [gateway](#gateway)
- [plugins](#plugins)
- [session](#session)
- [cron](#cron)
- [models](#models)
- [env](#env)
- [watchdog](#watchdog)
- [heartbeat](#heartbeat-config)
- [Additional sections](#additional-sections)

---

## agents

Contains `defaults` (inherited by all agents) and `list` (per-agent definitions).

### agents.defaults

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `model.primary` | string | `"claude-opus-4-6"` | Primary model ID |
| `model.fallbacks` | string[] | `[]` | Fallback model IDs, tried in order |
| `bootstrapMaxTokens` | number | `40000` | Max tokens for bootstrap context injection |
| `userTimezone` | string | `"UTC"` | IANA timezone for time-aware prompts |
| `contextTokens` | number | `200000` | Context window budget |
| `maxOutputTokens` | number | `16384` | Max tokens per response |
| `timeoutSeconds` | number | `300` | LLM call timeout |
| `workspace` | string | -- | Default workspace path |
| `compaction` | object | see below | History compaction settings |
| `routing` | object | see below | Model routing/tiering |
| `heartbeat` | object | -- | Default heartbeat config |
| `tools` | object | see below | Default tool profile |
| `approval.mode` | `"autonomous"` \| `"guarded"` \| `"supervised"` | `"autonomous"` | Tool approval mode |
| `narrationFilter` | boolean | `true` | Filter narration from output |
| `pathGuard` | boolean | `true` | Restrict filesystem access to workspace + allowedRoots |
| `allowedRoots` | string[] | `[]` | Additional filesystem paths the agent may access |

**Backwards compat:** `bootstrapMaxChars` is silently migrated to `bootstrapMaxTokens`.

#### agents.defaults.compaction

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | `"default"` \| `"safeguard"` | `"default"` | Compaction strategy |
| `reserveTokensFloor` | number | `8000` | Minimum tokens reserved after compaction |
| `maxHistoryShare` | number | `0.7` | Max fraction of context window for history |
| `distillationModel` | string | `"claude-haiku-4-5-20251001"` | Model for distillation summaries |
| `preserveRecentMessages` | number | `10` | Recent messages exempt from compaction |
| `preserveRecentMaxTokens` | number | `12000` | Token cap for preserved recent messages |
| `memoryFlush.enabled` | boolean | `true` | Flush to long-term memory before compaction |
| `memoryFlush.softThresholdTokens` | number | `8000` | Token threshold triggering soft flush |
| `memoryFlush.prompt` | string | -- | Custom extraction prompt |
| `memoryFlush.systemPrompt` | string | -- | Custom system prompt for extraction |

#### agents.defaults.routing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable model routing by complexity tier |
| `tiers.routine` | string | `"claude-haiku-4-5-20251001"` | Model for routine tasks |
| `tiers.standard` | string | `"claude-sonnet-4-6"` | Model for standard tasks |
| `tiers.complex` | string | `"claude-sonnet-4-6"` | Model for complex tasks |
| `agentOverrides` | Record<string, tier> | `{}` | Force a tier for specific agent IDs |

#### agents.defaults.tools

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `profile` | `"minimal"` \| `"coding"` \| `"messaging"` \| `"full"` | `"full"` | Base tool profile |
| `allow` | string[] | `[]` | Additional tool names to allow |
| `deny` | string[] | `[]` | Tool names to deny |

### agents.list[]

Each entry defines a nous (agent).

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Unique agent identifier |
| `default` | boolean | no | `false` | Default agent for unrouted messages |
| `name` | string | no | -- | Display name |
| `workspace` | string | yes | -- | Absolute path to agent workspace |
| `model` | string \| object | no | inherits | Per-agent model override. String or `{ primary, fallbacks }` |
| `params` | object | no | -- | LLM params: `maxTokens`, `temperature`, `thinkingBudget` (passthrough) |
| `subagents.allowAgents` | string[] | no | `[]` | Agent IDs this agent can spawn |
| `subagents.model` | string \| object | no | -- | Model for spawned subagents |
| `tools` | object | no | inherits | Per-agent tool profile override |
| `heartbeat` | object | no | -- | Per-agent heartbeat override |
| `identity.name` | string | no | -- | Identity name for prompts |
| `identity.emoji` | string | no | -- | Emoji prefix for Signal messages |
| `allowedRoots` | string[] | no | -- | Per-agent filesystem access roots |
| `domains` | string[] | no | -- | Domain tags for the agent |

Extra fields are preserved (passthrough).

```json
{
  "id": "research",
  "name": "Scholar",
  "workspace": "/path/to/aletheia/nous/scholar",
  "model": "claude-sonnet-4-6",
  "identity": { "name": "Scholar", "emoji": "📚" }
}
```

```yaml
# Rust equivalent
agents:
  list:
    - id: research
      name: Scholar
      workspace: /path/to/aletheia/instance/nous/scholar
      model: claude-sonnet-4-6
      identity:
        name: Scholar
        emoji: "📚"
```

---

## bindings

Array of routing rules mapping channels/peers to agents. Evaluated in order - first match wins.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `agentId` | string | yes | Target agent ID |
| `match.channel` | string | yes | Channel name (e.g. `"signal"`) |
| `match.accountId` | string | no | Restrict to specific channel account |
| `match.peer.kind` | string | no | `"dm"` or `"group"` |
| `match.peer.id` | string | no | Peer identifier (Signal UUID or group ID) |

```json
{
  "agentId": "main",
  "match": {
    "channel": "signal",
    "peer": { "kind": "dm", "id": "abc-123-uuid" }
  }
}
```

A binding with only `channel` (no peer) acts as a catch-all. More specific bindings should appear first.

---

## channels

### channels.signal

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Enable Signal channel |
| `accounts` | Record<string, account> | `{}` | Named account configs |

**Flat format (v1 compat):** If `account` exists at top level without `accounts`, it is lifted into `{ accounts: { default: { ... } } }`.

### channels.signal.accounts.*

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | -- | Display name |
| `enabled` | boolean | `true` | Enable this account |
| `account` | string | -- | Phone number (e.g. `"+15551234567"`) |
| `httpUrl` | string | -- | Full URL override for signal-cli REST API |
| `httpHost` | string | `"localhost"` | signal-cli REST API host |
| `httpPort` | number | `8080` | signal-cli REST API port |
| `cliPath` | string | -- | Path to signal-cli binary |
| `autoStart` | boolean | `true` | Auto-start receive loop |
| `receiveMode` | `"on-start"` \| `"manual"` | `"on-start"` | When to start receiving |
| `sendReadReceipts` | boolean | `true` | Send read receipts |
| `dmPolicy` | `"pairing"` \| `"allowlist"` \| `"open"` \| `"disabled"` | `"open"` | DM access policy |
| `groupPolicy` | `"open"` \| `"disabled"` \| `"allowlist"` | `"allowlist"` | Group access policy |
| `allowFrom` | (string \| number)[] | `[]` | Allowed sender IDs (for `allowlist` policy) |
| `groupAllowFrom` | (string \| number)[] | `[]` | Allowed group IDs (for `allowlist` policy) |
| `textChunkLimit` | number | `2000` | Max chars per outgoing message chunk |
| `mediaMaxMb` | number | `25` | Max media attachment size (MB) |
| `requireMention` | boolean | `true` | Require @mention in groups |

**DM policies:**

| Policy | Behavior |
|--------|----------|
| `pairing` | New contacts must complete a challenge-code handshake |
| `allowlist` | Only UUIDs in `allowFrom` can send DMs |
| `open` | Anyone can send DMs |
| `disabled` | DMs ignored |

```json
"signal": {
  "enabled": true,
  "accounts": {
    "default": {
      "account": "+15551234567",
      "dmPolicy": "pairing",
      "groupPolicy": "allowlist",
      "groupAllowFrom": ["group-id-abc"]
    }
  }
}
```

### channels.slack

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable Slack channel |
| `mode` | `"socket"` \| `"http"` | `"socket"` | Connection mode |
| `appToken` | string | -- | Socket Mode token (`xapp-...`) |
| `botToken` | string | -- | Bot User OAuth token (`xoxb-...`) |
| `signingSecret` | string | -- | HTTP mode signing secret |
| `dmPolicy` | `"open"` \| `"allowlist"` \| `"pairing"` \| `"disabled"` | `"open"` | DM access policy |
| `groupPolicy` | `"open"` \| `"allowlist"` \| `"disabled"` | `"allowlist"` | Channel access policy |
| `allowedChannels` | string[] | `[]` | Allowed Slack channel IDs |
| `allowedUsers` | string[] | `[]` | Allowed Slack user IDs |
| `requireMention` | boolean | `true` | Require @mention in channels |
| `streaming` | boolean | `true` | Stream responses |

---

## gateway

HTTP gateway serving the API, MCP endpoint, and web UI.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | number | `18789` | Listen port |
| `bind` | `"auto"` \| `"lan"` \| `"loopback"` \| `"custom"` | `"lan"` | Bind address strategy |
| `auth.mode` | `"none"` \| `"token"` \| `"password"` \| `"session"` | `"token"` | Authentication mode |
| `auth.token` | string \| SecretRef | -- | Bearer token (auto-generated if absent) |
| `auth.users` | array | `[]` | User accounts (for `password`/`session` modes) |
| `auth.session.accessTokenTtl` | number | `900` | Access token TTL (seconds) |
| `auth.session.refreshTokenTtl` | number | `2592000` | Refresh token TTL (seconds) |
| `auth.session.maxSessionsPerUser` | number | `10` | Max concurrent sessions per user |
| `auth.session.secureCookies` | boolean | `true` | Require HTTPS for cookies |
| `controlUi.enabled` | boolean | `true` | Serve web UI at `/ui` |
| `controlUi.allowInsecureAuth` | boolean | `false` | Allow auth over plain HTTP |
| `mcp.requireAuth` | boolean | `true` | Require auth for MCP endpoint |
| `rateLimit.requestsPerMinute` | number | `60` | API rate limit |
| `cors.allowOrigins` | string[] | `[]` | CORS allowed origins |
| `maxBodyBytes` | number | `1048576` | Max request body size (bytes) |

**Bind modes:**

| Mode | Behavior |
|------|----------|
| `auto` | LAN if available, falls back to loopback |
| `lan` | Binds to LAN interface |
| `loopback` | `127.0.0.1` only |
| `custom` | Custom bind address |

```json
"gateway": {
  "port": 18789,
  "bind": "loopback",
  "auth": { "mode": "token", "token": "my-secret-token" },
  "controlUi": { "enabled": true }
}
```

---

## plugins

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Global plugin toggle |
| `load.paths` | string[] | `[]` | Directories to scan. Relative paths resolve from repo root. |
| `entries` | Record<string, entry> | `{}` | Per-plugin overrides |
| `entries.*.enabled` | boolean | `true` | Enable/disable specific plugin |
| `entries.*.config` | Record<string, unknown> | `{}` | Plugin-specific config passed to init |

The loader looks for `manifest.json` or `*.plugin.json` in each path.

```json
"plugins": {
  "enabled": true,
  "load": {
    "paths": ["infrastructure/memory/aletheia-memory"]
  },
  "entries": {
    "aletheia-memory": {
      "enabled": true,
      "config": { "sidecarUrl": "http://localhost:8230" }
    }
  }
}
```

---

## session

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `scope` | `"per-sender"` \| `"global"` | `"per-sender"` | Session isolation strategy |
| `store` | string | -- | Custom path to sessions SQLite DB |
| `idleMinutes` | number | `120` | Inactivity timeout before session expires |
| `mainKey` | string | `"main"` | Session key for the primary session |
| `agentToAgent.maxPingPongTurns` | number | `5` | Max turns in agent-to-agent conversations |

| Scope | Behavior |
|-------|----------|
| `per-sender` | Each sender gets an independent session per agent |
| `global` | All senders share a single session per agent |

---

## cron

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Global cron toggle |
| `jobs` | CronJob[] | `[]` | Job definitions |

### cron.jobs[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Unique job identifier |
| `enabled` | boolean | no | `true` | Enable/disable |
| `name` | string | no | -- | Display name |
| `schedule` | string | yes | -- | Cron expression (e.g. `"0 2 * * *"`) |
| `agentId` | string | no | -- | Agent to run the job (default agent if omitted) |
| `sessionKey` | string | no | -- | Session key override |
| `model` | string | no | -- | Model override |
| `messageTemplate` | string | no | -- | Message sent to agent when job fires |
| `command` | string | no | -- | Shell command (alternative to messageTemplate) |
| `timeoutSeconds` | number | no | `300` | Execution timeout |

A job must have either `messageTemplate` or `command`, not both.

```json
"cron": {
  "enabled": true,
  "jobs": [
    {
      "id": "nightly-consolidation",
      "schedule": "0 2 * * *",
      "agentId": "main",
      "messageTemplate": "Run nightly memory consolidation."
    },
    {
      "id": "health-check",
      "schedule": "*/45 * * * *",
      "agentId": "main",
      "messageTemplate": "Heartbeat check-in. Report any issues.",
      "model": "claude-haiku-4-5-20251001"
    }
  ]
}
```

---

## models

Custom model provider definitions for non-Anthropic providers or custom endpoints.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `providers` | Record<string, provider> | `{}` | Named provider configs |

### models.providers.*

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `baseUrl` | string \| SecretRef | yes | -- | API base URL |
| `apiKey` | string \| SecretRef | no | -- | API key |
| `auth` | `"api-key"` \| `"oauth"` \| `"token"` | no | `"api-key"` | Auth method |
| `api` | `"anthropic-messages"` \| `"openai-completions"` \| `"google-generative-ai"` | no | `"anthropic-messages"` | API protocol |
| `models` | ProviderModel[] | no | `[]` | Model definitions |

### models.providers.*.models[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Model identifier |
| `name` | string | yes | -- | Display name |
| `reasoning` | boolean | no | `false` | Supports extended thinking |
| `input` | (`"text"` \| `"image"`)[] | no | `["text"]` | Input modalities |
| `contextWindow` | number | yes | -- | Context window (tokens) |
| `maxTokens` | number | yes | -- | Max output tokens |

```json
"models": {
  "providers": {
    "local-ollama": {
      "baseUrl": "http://localhost:11434/v1",
      "auth": "api-key",
      "api": "openai-completions",
      "models": [
        {
          "id": "llama3:70b",
          "name": "Llama 3 70B",
          "contextWindow": 8192,
          "maxTokens": 4096
        }
      ]
    }
  }
}
```

---

## env

Environment variables injected into the runtime and child processes.

**Flat (preferred):**

```json
"env": {
  "PATH": "/path/to/aletheia/shared/bin",
  "ALETHEIA_ROOT": "/path/to/aletheia"
}
```

**Structured:**

```json
"env": {
  "vars": {
    "PATH": "<aletheia-root>/shared/bin"
  }
}
```

If the top-level object has no `vars` key, it is treated as flat format and wrapped automatically.

---

## watchdog

Health monitoring for dependent services.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Enable watchdog |
| `intervalMs` | number | `300000` (5 min) | Check interval (ms) |
| `alertRecipient` | string | -- | Signal UUID to receive alerts |
| `services` | WatchdogService[] | `[]` | Services to monitor |

### watchdog.services[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | -- | Service name |
| `url` | string | yes | -- | Health check URL (expects 2xx) |
| `timeoutMs` | number | no | `3000` | Request timeout |

```json
"watchdog": {
  "enabled": true,
  "intervalMs": 300000,
  "alertRecipient": "signal-uuid-here",
  "services": [
    { "name": "memory-sidecar", "url": "http://localhost:8230/health" },
    { "name": "qdrant", "url": "http://localhost:6333/healthz" }
  ]
}
```

---

## Heartbeat Config

Used in `agents.defaults.heartbeat` or per-agent `heartbeat`. Sends periodic check-in messages.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `every` | string | `"45m"` | Interval (duration string) |
| `activeHours.start` | string | `"08:00"` | Active window start (24h) |
| `activeHours.end` | string | `"23:00"` | Active window end |
| `activeHours.timezone` | string | `"UTC"` | Timezone for active hours |
| `model` | string | -- | Model override for heartbeat calls |
| `session` | string | `"main"` | Session key for heartbeat messages |
| `prompt` | string | -- | Custom heartbeat prompt |

---

## Additional Sections

The schema defines several additional top-level sections not fully documented here. See `src/taxis/schema.ts` for complete definitions:

| Section | Purpose |
|---------|---------|
| `branding` | Instance name, tagline, favicon |
| `mcp` | MCP server definitions (stdio/http/sse transports) |
| `privacy` | Retention policies, PII detection/masking |
| `sandbox` | Tool execution sandboxing (Docker or pattern-only) |
| `encryption` | At-rest encryption for session data |
| `backup` | Automated backup destination and retention |
| `updates` | Update channel (stable/edge) and auto-check |
| `planning` | Dianoia planning system tuning |
| `memoryHealth` | Memory subsystem health thresholds |

---

## Minimal Config

### JSON (TypeScript runtime)

```json
{
  "agents": {
    "defaults": {
      "model": { "primary": "claude-sonnet-4-6" }
    },
    "list": [
      {
        "id": "main",
        "default": true,
        "workspace": "/path/to/workspace"
      }
    ]
  }
}
```

### YAML (Rust)

```yaml
agents:
  defaults:
    model:
      primary: claude-sonnet-4-6
  list:
    - id: main
      default: true
      workspace: /path/to/instance/nous/main
```

Everything else has sensible defaults. Add sections as needed.
