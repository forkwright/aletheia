# Network call inventory

Every network connection Aletheia makes, documented for transparency.

> **Zero unsolicited outbound connections.**
> Aletheia makes no telemetry, crash reporting, update checking, license validation, or heartbeat calls. The only outbound connections are to services you explicitly configure or trigger.

---

## Outbound connections

Sorted by criticality (highest sovereignty impact first).

| Endpoint | Protocol | Direction | Carries | Triggered by | Source reference |
|----------|----------|-----------|---------|--------------|------------------|
| `api.anthropic.com` | HTTPS | Outbound | Conversation messages, system prompts, tool call results | Every LLM inference call (streaming or non-streaming) | `crates/hermeneus/src/models.rs:4` (default URL); `crates/hermeneus/src/anthropic/client.rs:323` (streaming POST); `crates/hermeneus/src/anthropic/client.rs:761` (non-streaming POST) |
| `claude` CLI subprocess → Anthropic (indirect) | stdio (spawns `claude`, which manages its own HTTPS) | Indirect outbound | Prompts, conversation context, OAuth tokens (handled by CC) | LLM call when `CcProvider` is configured | `crates/hermeneus/src/cc/provider.rs:1` (architecture); `crates/hermeneus/src/cc/process.rs:110` (subprocess spawn) |
| Anthropic server-side `web_search` | HTTPS (inside Anthropic API response) | Indirect outbound | Search queries, result snippets, citations | LLM response requests a `web_search` server tool use | `crates/organon/src/types/context.rs:38` (catalog entry); `crates/organon/src/types/context.rs:65` (tool definition); `crates/nous/src/execute/mod.rs:118` (response extraction) |
| `console.anthropic.com` | HTTPS | Outbound | Refresh token, access token, OAuth client credentials | Background refresh when OAuth credential nears expiry; or `aletheia credential refresh` | `crates/symbolon/src/credential/mod.rs:48` (endpoint constant); `crates/symbolon/src/credential/refresh.rs:471` (token refresh POST) |
| Configurable OAuth provider (PKCE / device code) | HTTPS | Outbound | Authorization codes, device codes, access tokens | Library functions `pkce_login` / device code flow in `crates/symbolon` (not yet exposed as CLI commands) | `crates/symbolon/src/credential/pkce.rs:361` (authorization URL build); `crates/symbolon/src/credential/pkce.rs:612` (code→token exchange); `crates/symbolon/src/credential/device_code.rs:220` (device authorization POST); `crates/symbolon/src/credential/device_code.rs:285` (token polling POST) |
| HuggingFace Hub (`hf-hub` internal endpoint) | HTTPS (via `ureq`) | Outbound | Model files: `config.json`, `tokenizer.json`, `model.safetensors` | First initialization of the `candle` embedding provider | `crates/episteme/src/embedding.rs:165` (hub API init); `crates/episteme/src/embedding.rs:173` (`config.json` download); `crates/episteme/src/embedding.rs:179` (`tokenizer.json` download); `crates/episteme/src/embedding.rs:185` (`model.safetensors` download) |
| `api.github.com` | HTTPS | Outbound | Public issue metadata (title, state, labels, milestone) | Agent uses `issue_scan` or `issue_triage` built-in tool | `crates/organon/src/builtins/triage/mod.rs:359` (URL template); `crates/organon/src/builtins/triage/mod.rs:369` (HTTP GET) |
| Arbitrary URLs (`web_fetch` tool) | HTTPS/HTTP | Outbound | Arbitrary web page body (HTML, markdown, etc.) | Agent uses `web_fetch` built-in tool | `crates/organon/src/builtins/research.rs:130` (protocol guard); `crates/organon/src/builtins/research.rs:77` (HTTP GET execution); `crates/organon/src/builtins/research.rs:141` (automatic redirects disabled) |
| Operator-configured external tool endpoints | HTTPS/HTTP (config-driven) | Outbound | JSON tool body (`tool`, `kind`, `arguments`) | Agent invokes an external tool registered in `aletheia.toml` | `crates/aletheia/src/external_tools.rs:30` (config types); `crates/aletheia/src/external_tools.rs:330` (HTTP POST executor) |
| Qdrant (migration only) | HTTPS/gRPC (configurable, default `localhost:6333`) | Outbound | Vector memory records (scroll points, payloads) | One-time `aletheia migrate-memory` CLI command | `crates/aletheia/src/migrate_memory.rs:68` (client build); `crates/aletheia/src/commands/agent_io.rs:106` (CLI `--qdrant-url` arg) |
| signal-cli daemon (`localhost:8080` by default) | HTTP (JSON-RPC) | Outbound | Signal message text, recipient IDs, JSON-RPC envelopes | Sending or receiving Signal messages when Signal channel is enabled | `crates/taxis/src/config/mod.rs:268` (default host); `crates/agora/src/semeion/client.rs:88` (URL construction); `crates/agora/src/semeion/client.rs:144` (`send` RPC); `crates/agora/src/semeion/mod.rs:170` (receive poll loop) |
| Tailscale local status query | stdio (`tailscale status --json`) | Outbound (local-only) | Local Tailscale IPv4 address | Pylon server startup (discovery file write) | `crates/pylon/src/discovery.rs:125` (subprocess spawn) |

### Usage categories

- **Always-on (when configured):**
  - `api.anthropic.com` - every agent turn if Anthropic is the provider.
  - signal-cli daemon - when the Signal channel is enabled.
  - `console.anthropic.com` - background OAuth refresh when using Claude Code credentials.

- **Operator-triggered:**
  - `claude` CLI subprocess - triggered by LLM calls when CC provider is selected.
  - Anthropic server-side `web_search` - triggered by the LLM during inference.
  - Configurable OAuth PKCE / device code - triggered by provider-specific login flows (library-level; no CLI command exposed yet).
  - `api.github.com` - triggered by `issue_scan` / `issue_triage` tool use.
  - Arbitrary URLs (`web_fetch`) - triggered by `web_fetch` tool use.
  - External tool endpoints - triggered by configured external tool invocations.

- **One-time:**
  - HuggingFace Hub - first download when the `candle` embedding provider initializes; cached thereafter.
  - Qdrant - only during `migrate-memory` CLI command (`migrate-qdrant` feature).
  - Tailscale local status query - once per pylon server startup.

### Configurable / opt-out

| Endpoint | Configurable? | Opt-out method |
|----------|--------------|----------------|
| `api.anthropic.com` | Yes | Set a different provider base URL in provider config, or use local LLM (CC CLI, Ollama, OpenAI-compatible) |
| `claude` CLI subprocess | Yes | Do not select `cc` provider; use direct API key or another provider |
| Anthropic server-side `web_search` | Yes | Do not enable `web_search` in server tool config |
| `console.anthropic.com` | Partial (URL is hard-coded for Anthropic OAuth) | Do not use OAuth credentials; use static API key instead |
| Configurable OAuth PKCE / device code | Partial (URLs are configurable per provider) | Use a static API key; PKCE / device-code flows are not exposed as CLI commands |
| HuggingFace Hub | Partial (model repo is configurable) | Use a pre-cached model directory (`HF_HOME`), or air-gap the host |
| `api.github.com` | No (host is hard-coded) | Do not enable or invoke `issue_scan` / `issue_triage` |
| Arbitrary URLs (`web_fetch`) | No (arbitrary by design) | Do not invoke `web_fetch`; SSRF guards reject blocked hostnames and URLs that resolve to internal ranges |
| External tool endpoints | Yes (operator must explicitly declare them in `aletheia.toml`) | Remove the tool from config |
| Qdrant | Yes (`--qdrant-url` or `QDRANT_URL` env) | Do not run `migrate-memory`; feature is off by default |
| signal-cli daemon | Yes (`channels.signal.accounts.*.http_host`, `http_port`) | Disable Signal channel or set `enabled = false` |
| Tailscale local status query | No | Do not install `tailscale` binary; discovery gracefully degrades to `localhost` |

---

## Arbitrary URL SSRF guard

`web_fetch` and `http_request` validate the initial URL before sending a request. They reject blocked hostnames such as `localhost` and `metadata.google.internal`, and they reject hosts whose DNS resolution returns private, loopback, link-local, or cloud metadata addresses.

Both tools disable reqwest automatic redirects. They follow redirects manually, revalidate every `Location` target with the same hostname and DNS policy before the next request, and refuse the sixth redirect in a chain. Relative redirect targets are resolved against the current URL before validation.

Known limitation: DNS can change after validation and before the subsequent TCP connection. The guard reduces SSRF exposure by validating each URL the process chooses to request, but it does not pin the validated address through connect time.

## Inbound connections

| Listener | Protocol | Default Port | Purpose | Source reference |
|----------|----------|-------------|---------|------------------|
| Pylon HTTP gateway | HTTP/HTTPS | 18789 | REST API, SSE streams, OpenAPI docs, Prometheus metrics | `crates/pylon/src/server.rs:218` (TCP bind); `crates/pylon/src/router.rs:117` (API route nest) |
| PKCE OAuth callback (transient) | HTTP | random ephemeral loopback port | Receives OAuth authorization code during a PKCE OAuth login flow (library-level; not exposed as a CLI command) | `crates/symbolon/src/credential/pkce.rs:496` (local TCP bind) |

---

## Local-only components

These components make **no network calls**:

- **candle**: pure Rust inference for embeddings, runs entirely in-process
- **mneme**: embedded graph + vector database, no network protocol
- **fjall**: session and knowledge stores, file-based
- **Prometheus metrics**: passive endpoint (`GET /metrics`), scraped by external collector
- **Configuration loading**: reads local YAML/TOML files only

---

## Anthropic data sovereignty (defaults)

The `hermeneus` crate owns the Anthropic client boundary. Every outbound
request is scrubbed before it leaves the process:

| Control | Default | Issue | Behaviour |
|---------|---------|-------|-----------|
| Training opt-out header | Always on | #3406 | `anthropic-disable-training: true` and `anthropic-training-opt-out: true` are sent on every request. Not configurable - sovereignty default. |
| Prompt cache markers | Disabled | #3410 | No `cache_control` markers on any block. Operator system prompts, tool definitions, and conversation history never enter Anthropic's prompt cache. Opt in via `[anthropic] promptCacheMode = "ephemeral"` if the cost tradeoff is acceptable. |
| CC attribution fingerprint | Stripped | #3409 | The 3-char fingerprint slot in the attribution block is pinned to `000`; the upstream CC algorithm would otherwise hash operator message content into it. |
| `X-Claude-Code-Session-Id` | Randomized per request | #3409 | Fresh UUID on every call so Anthropic cannot correlate requests to a persistent operator session. Upstream CC sends a stable per-process UUID. |
| `User-Agent`, `anthropic-beta`, `anthropic-version`, `x-app` | Preserved | - | Required for OAuth tier access; values are static and do not carry operator identity. |

## No telemetry

Aletheia makes zero unsolicited outbound network connections. There is no:

- Usage analytics or telemetry
- Crash reporting
- Update checking or phone-home
- License validation
- Beacon or heartbeat to any external service

The only outbound connections are to services you explicitly configure (LLM provider, Signal) or trigger (tool calls, OAuth login, model download). Crates that use `reqwest`:

- `hermeneus`: LLM provider API calls (Anthropic, etc.)
- `agora`: Signal JSON-RPC (localhost)
- `symbolon`: OAuth token refresh/validation
- `organon`: tool execution (`web_fetch`, `web_search` wiring, GitHub triage)
- `nous`: pipeline HTTP calls and server-tool tracking
- `aletheia`: binary entry point (health checks, eval runner, external tool proxy, Qdrant migration)
- `eval` (dokimion): behavioral eval HTTP scenario runner
- `tui`: terminal dashboard API client

All reqwest usage targets either the configured LLM provider, localhost services, or user-initiated tool calls. No unsolicited outbound connections.

---

## Firewall rules

Minimum rules for a working deployment:

| Direction | Destination | Port | Required |
|-----------|------------|------|----------|
| Outbound | `api.anthropic.com` | 443 | Yes (for Anthropic LLM) |
| Outbound | `console.anthropic.com` | 443 | Only if using OAuth credentials |
| Outbound | `localhost` | 8080 | Only if Signal enabled |
| Outbound | `*.huggingface.co` / CDN | 443 | Only on first `candle` init (until cached) |
| Outbound | `api.github.com` | 443 | Only if using GitHub triage tools |
| Outbound | Arbitrary HTTPS | 443 | Only if `web_fetch` or external tools are used |
| Outbound | Operator-configured tool endpoint | varies | Only if external tools are configured |
| Outbound | Qdrant URL | 6333 (default) | Only during `migrate-memory` |
| Inbound | `*` | 18789 | For API/UI access |

Air-gapped operation is possible with:
- a local LLM provider (configurable base URL),
- pre-cached HuggingFace models (`HF_HOME`),
- Signal disabled,
- no external tools configured,
- static API key (no OAuth).

---

## Data flow diagram

```text
                    INBOUND                              OUTBOUND
                    -------                              --------

  Signal app ──E2E──▶ Signal servers ──E2E──▶ signal-cli daemon
                                                    │
  Web browser ──HTTP──▶ pylon (:18789)              │ (localhost only)
                            │                       │
  curl / API ──HTTP──▶ pylon (:18789)               │
                            │                       │
                   ┌────────┴─────────┐             │
                   │  Channel Router   │◀────────────┘
                   │  (bindings)       │
                   └────────┬─────────┘
                            │
                   ┌────────┴─────────┐
                   │   NousActor      │
                   │   (pipeline)     │
                   │                  │     ┌──────────────────┐
                   │   execute ───────│────▶│  Anthropic API   │
                   │                  │     │  (HTTPS, outbound│
                   │   finalize       │     │  only connection)│
                   └────────┬─────────┘     └──────────────────┘
                            │
               ┌────────────┼────────────┐
               │            │            │
      ┌────────┴──┐   ┌────┴─────┐  ┌───┴────────┐
      │  mneme    │   │  fjall   │  │  candle     │
      │ (knowledge│   │ (sessions│  │ (embeddings)│
      │  graphs)  │   │  .db)    │  │             │
      └───────────┘   └──────────┘  └─────────────┘
            LOCAL          LOCAL          LOCAL
```

### What leaves the system

- **User messages and system prompts** → sent to Anthropic API (or delegated to Claude Code CLI) for inference
- **Signal messages** → routed through Signal protocol (E2E encrypted) via local signal-cli daemon
- **OAuth tokens** → refreshed against `console.anthropic.com` when using Claude Code credentials
- **Search queries** → sent server-side by Anthropic when `web_search` is enabled and used
- **GitHub issue metadata** → fetched from `api.github.com` during triage tool execution
- **Arbitrary URLs** → fetched by `web_fetch` or external tool proxies when explicitly invoked
- **Model files** → downloaded from HuggingFace Hub on first `candle` initialization

### What stays local

- **Session history** -> fjall (`instance/data/sessions.db`, directory name kept for compatibility)
- **Working checkpoints** → fjall (`instance/data/working-checkpoints.fjall`)
- **Knowledge graphs and vectors** → mneme (embedded Datalog engine)
- **Embeddings** → computed locally by candle
- **Agent workspaces** → local filesystem (`instance/nous/`)
- **Trace logs** → local filesystem (`instance/logs/traces/`)
- **Backups** → local filesystem (`instance/data/backups/`)
- **Prometheus metrics** → exposed on local port, never pushed

See [DATA.md](DATA.md) for the complete data inventory and retention policies.
