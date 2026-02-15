# Aletheia Configuration Reference

Config file location: `~/.aletheia/aletheia.json`

Validated at startup against the Zod schema in `src/taxis/schema.ts`. Unknown top-level fields are preserved (passthrough) for forward compatibility.

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

---

## agents

Contains `defaults` (inherited by all agents) and `list` (per-agent definitions).

### agents.defaults

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `model.primary` | string | `"claude-opus-4-6"` | Primary model ID |
| `model.fallbacks` | string[] | `[]` | Fallback model IDs tried in order |
| `bootstrapMaxTokens` | number | `40000` | Max tokens for bootstrap context injection |
| `userTimezone` | string | `"UTC"` | IANA timezone for time-aware prompts |
| `contextTokens` | number | `200000` | Context window budget |
| `maxOutputTokens` | number | `16384` | Max tokens per response |
| `timeoutSeconds` | number | `300` | LLM call timeout |
| `workspace` | string | _(none)_ | Default workspace path (optional) |
| `compaction` | object | _(see below)_ | History compaction settings |
| `routing` | object | _(see below)_ | Model routing/tiering |
| `heartbeat` | object | _(none)_ | Default heartbeat config (optional) |
| `tools` | object | _(see below)_ | Default tool profile |

**Backwards compat:** `bootstrapMaxChars` is silently migrated to `bootstrapMaxTokens`.

#### agents.defaults.compaction

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | `"default"` \| `"safeguard"` | `"default"` | Compaction strategy |
| `reserveTokensFloor` | number | `8000` | Minimum tokens reserved after compaction |
| `maxHistoryShare` | number | `0.7` | Max fraction of context window for history |
| `distillationModel` | string | `"claude-haiku-4-5-20251001"` | Model used for distillation summaries |
| `memoryFlush.enabled` | boolean | `true` | Flush to long-term memory before compaction |
| `memoryFlush.softThresholdTokens` | number | `8000` | Token threshold triggering soft flush |
| `memoryFlush.prompt` | string | _(none)_ | Custom extraction prompt (optional) |
| `memoryFlush.systemPrompt` | string | _(none)_ | Custom system prompt for extraction (optional) |

#### agents.defaults.routing

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `false` | Enable model routing by complexity tier |
| `tiers.routine` | string | `"claude-haiku-4-5-20251001"` | Model for routine/simple tasks |
| `tiers.standard` | string | `"claude-sonnet-4-5-20250929"` | Model for standard tasks |
| `tiers.complex` | string | `"claude-sonnet-4-5-20250929"` | Model for complex tasks |
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
| `default` | boolean | no | `false` | Is this the default agent for unrouted messages |
| `name` | string | no | _(none)_ | Display name |
| `workspace` | string | yes | -- | Absolute path to agent workspace |
| `model` | string \| object | no | _(inherits defaults)_ | Per-agent model override. String or `{ primary, fallbacks }` |
| `subagents` | object | no | `{}` | Subagent spawning config |
| `subagents.allowAgents` | string[] | no | `[]` | Agent IDs this agent can spawn as subagents |
| `subagents.model` | string \| object | no | _(none)_ | Model for spawned subagents |
| `tools` | object | no | _(inherits defaults)_ | Per-agent tool profile override |
| `heartbeat` | object | no | _(none)_ | Per-agent heartbeat override |
| `identity.name` | string | no | _(none)_ | Identity name used in prompts |
| `identity.emoji` | string | no | _(none)_ | Emoji prefix for Signal messages |

Extra fields are preserved (passthrough).

```json
{
  "id": "research",
  "name": "Scholar",
  "workspace": "/mnt/ssd/aletheia/nous/scholar",
  "model": "claude-sonnet-4-5-20250929",
  "identity": { "name": "Scholar", "emoji": "📚" }
}
```

---

## bindings

Array of routing rules mapping channels/peers to agents. Evaluated in order; first match wins.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `agentId` | string | yes | Target agent ID |
| `match.channel` | string | yes | Channel name (e.g. `"signal"`) |
| `match.accountId` | string | no | Restrict to specific channel account |
| `match.peer.kind` | string | no | Peer type: `"dm"` or `"group"` |
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

A binding with only `channel` (no peer) acts as a catch-all for that channel. More specific bindings should appear first.

---

## channels

### channels.signal

Top-level toggle and named accounts map.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Enable Signal channel |
| `accounts` | Record<string, account> | `{}` | Named account configs |

**Flat format (v1 compat):** If `account` exists at top level without `accounts`, it is lifted into `{ accounts: { default: { ... } } }`.

### channels.signal.accounts.*

Each account entry:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | _(none)_ | Display name (optional) |
| `enabled` | boolean | `true` | Enable this account |
| `account` | string | _(none)_ | Phone number (e.g. `"+15551234567"`) |
| `httpUrl` | string | _(none)_ | Full URL override for signal-cli REST API |
| `httpHost` | string | `"localhost"` | signal-cli REST API host |
| `httpPort` | number | `8080` | signal-cli REST API port |
| `cliPath` | string | _(none)_ | Path to signal-cli binary (optional) |
| `autoStart` | boolean | `true` | Auto-start receive loop |
| `receiveMode` | `"on-start"` \| `"manual"` | `"on-start"` | When to start receiving messages |
| `sendReadReceipts` | boolean | `true` | Send read receipts |
| `dmPolicy` | `"pairing"` \| `"allowlist"` \| `"open"` \| `"disabled"` | `"open"` | DM access policy |
| `groupPolicy` | `"open"` \| `"disabled"` \| `"allowlist"` | `"allowlist"` | Group message access policy |
| `allowFrom` | (string \| number)[] | `[]` | Allowed sender IDs for DMs (used with `allowlist` policy) |
| `groupAllowFrom` | (string \| number)[] | `[]` | Allowed group IDs (used with `allowlist` policy) |
| `textChunkLimit` | number | `2000` | Max characters per outgoing message chunk |
| `mediaMaxMb` | number | `25` | Max media attachment size in MB |
| `requireMention` | boolean | `true` | Require @mention in groups |

**DM policies:**

| Policy | Behavior |
|--------|----------|
| `pairing` | New contacts must complete a challenge-code handshake before messaging |
| `allowlist` | Only UUIDs in `allowFrom` can send DMs |
| `open` | Anyone can send DMs |
| `disabled` | DMs ignored entirely |

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

---

## gateway

HTTP gateway serving the API, MCP endpoint, and control UI.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | number | `18789` | Listen port |
| `bind` | `"auto"` \| `"lan"` \| `"loopback"` \| `"custom"` | `"lan"` | Bind address strategy |
| `auth.mode` | `"token"` \| `"password"` | `"token"` | Authentication mode |
| `auth.token` | string | _(none)_ | Bearer token for API auth (optional, auto-generated if absent) |
| `controlUi.enabled` | boolean | `true` | Serve web UI at `/ui` |
| `controlUi.allowInsecureAuth` | boolean | `false` | Allow auth over plain HTTP |

**Bind modes:**

| Mode | Behavior |
|------|----------|
| `auto` | Binds LAN if available, falls back to loopback |
| `lan` | Binds to LAN interface |
| `loopback` | Binds to `127.0.0.1` only |
| `custom` | Uses a custom bind address (set via additional config) |

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

Plugin loader configuration.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Global plugin toggle |
| `load.paths` | string[] | `[]` | Directories to scan for plugins. Relative paths resolve from aletheia root. |
| `entries` | Record<string, entry> | `{}` | Per-plugin overrides |
| `entries.*.enabled` | boolean | `true` | Enable/disable specific plugin |
| `entries.*.config` | Record<string, unknown> | `{}` | Plugin-specific config passed to its init |

The plugin loader looks for `manifest.json` or `*.plugin.json` in each path.

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

Conversation session management.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `scope` | `"per-sender"` \| `"global"` | `"per-sender"` | Session isolation strategy |
| `store` | string | _(none)_ | Custom path to sessions SQLite DB (optional) |
| `idleMinutes` | number | `120` | Minutes of inactivity before session expires |
| `mainKey` | string | `"main"` | Session key for the primary/default session |
| `agentToAgent.maxPingPongTurns` | number | `5` | Max back-and-forth turns in agent-to-agent conversations |

**Scope modes:**

| Scope | Behavior |
|-------|----------|
| `per-sender` | Each sender gets an independent session per agent |
| `global` | All senders share a single session per agent |

---

## cron

Scheduled jobs. Uses cron syntax.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | `true` | Global cron toggle |
| `jobs` | CronJob[] | `[]` | Job definitions |

### cron.jobs[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Unique job identifier |
| `enabled` | boolean | no | `true` | Enable/disable this job |
| `name` | string | no | _(none)_ | Display name |
| `schedule` | string | yes | -- | Cron expression (e.g. `"0 2 * * *"`) |
| `agentId` | string | no | _(none)_ | Agent to run the job (uses default agent if omitted) |
| `sessionKey` | string | no | _(none)_ | Session key override |
| `model` | string | no | _(none)_ | Model override for this job |
| `messageTemplate` | string | no | _(none)_ | Message sent to the agent when job fires |
| `command` | string | no | _(none)_ | Shell command to run instead of agent message |
| `timeoutSeconds` | number | no | `300` | Job execution timeout |

A job must have either `messageTemplate` (sends a message to an agent) or `command` (runs a shell command), not both.

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

Custom model provider definitions. Used to add non-Anthropic providers or custom endpoints.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `providers` | Record<string, provider> | `{}` | Named provider configs |

### models.providers.*

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `baseUrl` | string | yes | -- | API base URL |
| `apiKey` | string | no | _(none)_ | API key (optional if using other auth) |
| `auth` | `"api-key"` \| `"oauth"` \| `"token"` | no | `"api-key"` | Authentication method |
| `api` | `"anthropic-messages"` \| `"openai-completions"` \| `"google-generative-ai"` | no | `"anthropic-messages"` | API protocol |
| `models` | ProviderModel[] | no | `[]` | Model definitions for this provider |

### models.providers.*.models[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | -- | Model identifier used in config |
| `name` | string | yes | -- | Display name |
| `reasoning` | boolean | no | `false` | Model supports extended thinking |
| `input` | (`"text"` \| `"image"`)[] | no | `["text"]` | Supported input modalities |
| `contextWindow` | number | yes | -- | Context window size in tokens |
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

Supports two formats:

**Flat (preferred):**
```json
"env": {
  "PATH": "/mnt/ssd/aletheia/shared/bin",
  "ALETHEIA_ROOT": "/mnt/ssd/aletheia"
}
```

**Structured:**
```json
"env": {
  "vars": {
    "PATH": "/mnt/ssd/aletheia/shared/bin"
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
| `intervalMs` | number | `300000` (5 min) | Check interval in milliseconds |
| `alertRecipient` | string | _(none)_ | Signal UUID or identifier to receive alerts (optional) |
| `services` | WatchdogService[] | `[]` | Services to monitor |

### watchdog.services[]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | -- | Service display name |
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

Used in `agents.defaults.heartbeat` or per-agent `heartbeat`. Sends periodic check-in messages to keep agents warm.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `every` | string | `"45m"` | Interval (duration string) |
| `activeHours.start` | string | `"08:00"` | Quiet hours start (24h format) |
| `activeHours.end` | string | `"23:00"` | Quiet hours end |
| `activeHours.timezone` | string | `"UTC"` | Timezone for active hours |
| `model` | string | _(none)_ | Model override for heartbeat calls (optional) |
| `session` | string | `"main"` | Session key for heartbeat messages |
| `prompt` | string | _(none)_ | Custom heartbeat prompt (optional) |

---

## Minimal Config

The smallest working configuration:

```json
{
  "agents": {
    "defaults": {
      "model": { "primary": "claude-sonnet-4-5-20250929" }
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

Everything else has sensible defaults. Add sections as needed.
