# Network call inventory

Every outbound network connection Aletheia makes, documented for transparency.

---

## Outbound connections

| Destination | Protocol | Port | When | Data Sent | Configurable |
|-------------|----------|------|------|-----------|-------------|
| `api.anthropic.com` | HTTPS | 443 | Every LLM call | Conversation messages, system prompts, tool call results | `ANTHROPIC_API_KEY` env var; base URL via provider config |
| signal-cli daemon | HTTP (JSON-RPC) | 8080 | When Signal channel is enabled | Message text, recipient IDs | `channels.signal.accounts.*.http_host`, `http_port` |

## Local-only components

These components make **no network calls**:

- **candle**: pure Rust inference for embeddings, runs entirely in-process
- **mneme**: embedded graph + vector database, no network protocol
- **SQLite**: session store, file-based
- **Prometheus metrics**: passive endpoint (`GET /metrics`), scraped by external collector
- **Configuration loading**: reads local YAML file only

## Inbound connections

| Listener | Protocol | Default Port | Purpose |
|----------|----------|-------------|---------|
| Pylon HTTP gateway | HTTP/HTTPS | 18789 | REST API, SSE streams, OpenAPI docs, metrics |

---

## No telemetry

Aletheia makes zero unsolicited outbound network connections. There is no:

- Usage analytics or telemetry
- Crash reporting
- Update checking or phone-home
- License validation
- Beacon or heartbeat to any external service

The only outbound connections are to services you explicitly configure (LLM provider, Signal). Crates that use `reqwest`:

- `hermeneus`: LLM provider API calls (Anthropic, etc.)
- `agora`: Signal JSON-RPC (localhost)
- `symbolon`: OAuth token refresh/validation
- `organon`: tool execution (web_search, HTTP tools)
- `nous`: pipeline HTTP calls
- `aletheia`: binary entry point (health checks, eval runner)
- `eval` (dokimion): behavioral eval HTTP scenario runner
- `tui`: terminal dashboard API client

All reqwest usage targets either the configured LLM provider, localhost services, or user-initiated tool calls. No unsolicited outbound connections.

---

## Firewall rules

Minimum rules for a working deployment:

| Direction | Destination | Port | Required |
|-----------|------------|------|----------|
| Outbound | `api.anthropic.com` | 443 | Yes (for LLM) |
| Outbound | `localhost` | 8080 | Only if Signal enabled |
| Inbound | `*` | 18789 | For API/UI access |

Air-gapped operation is possible with a local LLM provider (configurable base URL in provider config).

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
      │  mneme    │   │  SQLite  │  │  candle     │
      │ (knowledge│   │ (sessions│  │ (embeddings)│
      │  graphs)  │   │  .db)    │  │             │
      └───────────┘   └──────────┘  └─────────────┘
            LOCAL          LOCAL          LOCAL
```

### What leaves the system

- **User messages and system prompts** → sent to Anthropic API for inference
- **Signal messages** → routed through Signal protocol (E2E encrypted)

### What stays local

- **Session history** → SQLite (`instance/data/sessions.db`)
- **Knowledge graphs and vectors** → mneme (embedded Datalog engine)
- **Embeddings** → computed locally by candle
- **Agent workspaces** → local filesystem (`instance/nous/`)
- **Trace logs** → local filesystem (`instance/logs/traces/`)
- **Backups** → local filesystem (`instance/data/backups/`)
- **Prometheus metrics** → exposed on local port, never pushed

See [DATA.md](DATA.md) for the complete data inventory and retention policies.
