# Network Call Inventory

Every outbound network connection Aletheia makes, documented for transparency.

---

## Outbound Connections

| Destination | Protocol | Port | When | Data Sent | Configurable |
|-------------|----------|------|------|-----------|-------------|
| `api.anthropic.com` | HTTPS | 443 | Every LLM call | Conversation messages, system prompts, tool call results | `ANTHROPIC_API_KEY` env var; base URL via provider config |
| signal-cli daemon | HTTP (JSON-RPC) | 8080 | When Signal channel is enabled | Message text, recipient IDs | `channels.signal.accounts.*.http_host`, `http_port` |

## Local-Only Components

These components make **no network calls**:

- **fastembed-rs** — ONNX inference for embeddings, runs entirely in-process
- **CozoDB** — embedded graph + vector database, no network protocol
- **SQLite** — session store, file-based
- **Prometheus metrics** — passive endpoint (`GET /metrics`), scraped by external collector
- **Configuration loading** — reads local YAML file only

## Inbound Connections

| Listener | Protocol | Default Port | Purpose |
|----------|----------|-------------|---------|
| Pylon HTTP gateway | HTTP/HTTPS | 18789 | REST API, web UI, SSE streams, OpenAPI docs, metrics |

---

## No Telemetry

Aletheia makes zero unsolicited outbound network connections. There is no:

- Usage analytics or telemetry
- Crash reporting
- Update checking or phone-home
- License validation
- Beacon or heartbeat to any external service

The only outbound connections are to services you explicitly configure (LLM provider, Signal). This is verifiable by inspecting the codebase — the only HTTP client (`reqwest`) usage is in `crates/hermeneus/` (LLM calls) and `crates/agora/` (Signal JSON-RPC).

---

## Firewall Rules

Minimum rules for a working deployment:

| Direction | Destination | Port | Required |
|-----------|------------|------|----------|
| Outbound | `api.anthropic.com` | 443 | Yes (for LLM) |
| Outbound | `localhost` | 8080 | Only if Signal enabled |
| Inbound | `*` | 18789 | For API/UI access |

Air-gapped operation is possible with a local LLM provider (configurable base URL in provider config).

---

## Data Flow Diagram

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
      │  CozoDB   │   │  SQLite  │  │ fastembed   │
      │ (knowledge│   │ (sessions│  │ (embeddings)│
      │  graphs)  │   │  .db)    │  │             │
      └───────────┘   └──────────┘  └─────────────┘
            LOCAL          LOCAL          LOCAL
```

### What Leaves the System

- **User messages and system prompts** → sent to Anthropic API for inference
- **Signal messages** → routed through Signal protocol (E2E encrypted)

### What Stays Local

- **Session history** → SQLite (`instance/data/sessions.db`)
- **Knowledge graphs and vectors** → CozoDB (embedded)
- **Embeddings** → computed locally by fastembed-rs
- **Agent workspaces** → local filesystem (`instance/nous/`)
- **Trace logs** → local filesystem (`instance/logs/traces/`)
- **Backups** → local filesystem (`instance/data/backups/`)
- **Prometheus metrics** → exposed on local port, never pushed

See [DATA.md](DATA.md) for the complete data inventory and retention policies.
