# Deployment

Production deployment reference. For configuration details, see [CONFIGURATION.md](CONFIGURATION.md). For upgrading an existing installation, see [UPGRADING.md](UPGRADING.md).

For first-time setup, see [QUICKSTART.md](QUICKSTART.md).

---

## System requirements

### Hardware

| | Minimum | Recommended |
|---|---|---|
| CPU | 2 cores | 4 cores |
| RAM | 2 GB | 4 GB (candle model loading) |
| Disk | 1 GB | 10 GB (session data growth) |

### Operating system

| Target | Notes |
|--------|-------|
| `x86_64-unknown-linux-gnu` | glibc-based Linux |
| `aarch64-unknown-linux-gnu` | ARM64 Linux (Raspberry Pi 4+, AWS Graviton) |
| `x86_64-apple-darwin` | macOS 12+ Intel |
| `aarch64-apple-darwin` | macOS 12+ Apple Silicon |

### Software

- Links dynamically against glibc only (Linux). No other runtime dependencies.
- **Build from source:** Rust 1.94+ (edition 2024), Cargo
- **Optional:** signal-cli for Signal messaging channel

### Network

- **Outbound:** HTTPS to your LLM provider (default: `api.anthropic.com:443`)
- **Inbound:** configurable listen port (default `18789`) for API
- **Local:** signal-cli JSON-RPC if Signal channel is enabled (default `localhost:8080`)

See [NETWORK.md](NETWORK.md) for the complete network call inventory.

---

## Installation

See [QUICKSTART.md](QUICKSTART.md) for standard install instructions (prebuilt binary and build from source).

### Headless build (no TUI)

The TUI (terminal dashboard) is compiled in by default. `--no-default-features` disables more than the TUI — the default set is `tui`, `recall`, `storage-fjall`, `embed-candle`, `cc-provider` — so pick the recipe that matches what you need:

```bash
# Full headless: all default functionality except the TUI
cargo build --release -p aletheia --no-default-features --features recall,storage-fjall,embed-candle,cc-provider,tls

# Minimal headless: HTTP API only — also drops recall wiring, candle ML, and the Claude Code provider
cargo build --release -p aletheia --no-default-features --features tls
```

Either way the Datalog engine and fjall storage code remain linked (`mneme` is a default-features dependency and `fjall` is unconditional). The headless binary accepts all the same CLI flags and API endpoints. The `aletheia status` command falls back to a plain-text summary when TUI is absent.

### Shell completions

Run `aletheia completions bash|zsh|fish` to generate shell completions.

---

## Instance setup

The instance directory holds all runtime state: config, databases, agent workspaces, logs. It is gitignored; the platform code ships without instance data.

### Recommended: use the init wizard

```bash
export ANTHROPIC_API_KEY="sk-ant-..."   # or enter interactively
aletheia init                            # interactive wizard, creates ./instance
```

The wizard prompts for API key, model, agent name, and bind address, then writes a fully valid `config/aletheia.toml`. For non-interactive (CI/scripting) use:

```bash
ANTHROPIC_API_KEY=sk-ant-... aletheia init --yes --instance-root /srv/aletheia/instance
```

### Manual: copy the example scaffold

Copy the example scaffold to create your instance directory:

```bash
cp -r instance.example instance
```

Then configure `instance/config/aletheia.toml` (see the Configuration section). The scaffold includes all required directories and example config files.

This creates the full directory scaffold:

```text
instance/
├── config/
│   ├── aletheia.toml       # Main config
│   ├── credentials/        # API keys, secrets
│   └── tls/                # TLS certs (optional)
├── data/                   # fjall stores and backups
├── logs/traces/            # Trace files
├── nous/                   # Agent workspaces
│   └── _template/          # Template for new agents
├── shared/coordination/    # Cross-agent coordination state
├── theke/                  # Human + agent collaborative space
├── signal/                 # signal-cli data (if using Signal)
```

See [instance.example/README.md](../instance.example/README.md) for the three-tier cascade and what goes where.

### Instance discovery

The binary finds the instance directory in this order:

1. `--instance-root` CLI flag (explicit path)
2. `ALETHEIA_ROOT` environment variable
3. `./instance` relative to the working directory

---

## Configuration

The init wizard writes a complete `config/aletheia.toml`. If you are setting up manually, create one from `instance.example/config/aletheia.toml`:

```bash
# If you didn't use the init wizard
cp instance.example/config/aletheia.toml instance/config/aletheia.toml
```

Then edit the file:

```toml
[gateway]
port = 18789
bind = "localhost"

[gateway.auth]
mode = "token"

[agents.defaults.model]
primary = "claude-sonnet-4-6"

[[agents.list]]
id = "main"
name = "Main"
default = true
workspace = "nous/main"
```

The workspace path can be relative (relative to the instance root) or absolute. In this example, `nous/main` is relative and will resolve to `./instance/nous/main` when the instance root is `./instance`. For absolute paths, use the full filesystem path: `/srv/aletheia/instance/nous/main`.

The config cascade loads in order (later wins): compiled defaults, TOML file, `ALETHEIA_` environment variables. See [CONFIGURATION.md](CONFIGURATION.md) for the complete reference.

---

## Authentication

### Auth modes

The gateway supports two authentication modes configured via `gateway.auth.mode`:

| Mode | Description | Use case |
|------|-------------|----------|
| `token` | Bearer token (JWT) authentication | Default, production deployments |
| `none` | No authentication | Development, local deployments |

### Token authentication (default)

When `gateway.auth.mode = "token"`, all requests to `/api/v1/` endpoints require an `Authorization: Bearer <token>` header.

Example request:

```bash
curl -H "Authorization: Bearer your-jwt-token" \
  http://127.0.0.1:18789/api/v1/sessions
```

The token is a JWT signed by the server. To obtain a token, use the CLI:

```bash
aletheia --instance-root ./instance credential status
```

This displays the current token or a way to generate one. Tokens are managed by the `aletheia` CLI and stored in `instance/config/credentials/`.

### POST/PUT/DELETE CSRF protection

Enable CSRF protection by adding to `aletheia.toml`:

```toml
[gateway.csrf]
enabled = true
```

When enabled, all state-changing requests (POST, PUT, DELETE, PATCH) to `/api/v1/` must include the header:

```
X-Requested-With: aletheia
```

Include this header on every mutating curl call when CSRF is enabled:

```bash
curl -X POST \
  -H "Authorization: Bearer your-token" \
  -H "X-Requested-With: aletheia" \
  -H "Content-Type: application/json" \
  -d '{"nous_id": "main", "session_key": "default"}' \
  http://127.0.0.1:18789/api/v1/sessions
```

Missing the CSRF header returns `403 Forbidden` when CSRF is enabled. Enable CSRF on any instance exposed to a network.

### No authentication mode

When `gateway.auth.mode = "none"`, the gateway accepts all requests without authentication:

```toml
[gateway.auth]
mode = "none"
```

**Security warning:** This mode is suitable only for local development. Never use in production or on exposed networks.

---

## Credentials

### LLM provider

Set the Anthropic API key as an environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

The binary reads `ANTHROPIC_API_KEY` from the environment at startup. If unset, it starts without an LLM provider (health check reports degraded status).

### TLS (optional)

Generate self-signed certificates for LAN use:

```bash
aletheia tls generate --output-dir instance/config/tls --days 365 --san localhost --san 192.168.1.100
```

Then enable in config:

```toml
[gateway.tls]
enabled = true
cert_path = "config/tls/cert.pem"
key_path = "config/tls/key.pem"
```

---

## Agent setup

### Recommended: use the init wizard

The `aletheia init` wizard covers agent setup as part of instance initialization. It prompts for agent name and creates the workspace and config entry automatically. If you used `aletheia init`, your first agent is already configured. Skip to [First run](#first-run).

### Manual: add an additional agent

To add a second agent or to configure one by hand:

1. Create a workspace from the template:

```bash
cp -r instance/nous/_template instance/nous/main
```

2. Edit the bootstrap files in the new workspace:
   - `SOUL.md`: agent identity and character
   - `IDENTITY.md`: display name, emoji
   - `GOALS.md`: current goals
   - `MEMORY.md`: persistent operational memory

3. Register the agent in `aletheia.toml`:

```toml
[[agents.list]]
id = "main"
default = true
```

> **Manual alternative:** Steps 1–3 in [Manual: add an additional agent](#manual-add-an-additional-agent) apply to both first-time manual setups and adding agents to an existing instance. If you used the init wizard, only step 3 may be needed to add additional agents.

---

## First run

```bash
aletheia --instance-root ./instance
```

Or with environment variable:

```bash
export ALETHEIA_ROOT=./instance
aletheia
```

For the full startup sequence (what the binary does at launch), see [ARCHITECTURE-GUIDE.md](ARCHITECTURE-GUIDE.md#the-binary).

### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `-r, --instance-root` | auto-discover | Path to instance directory |
| `-l, --log-level` | `info` | Log level (trace, debug, info, warn, error) |
| `--bind` | `localhost` | Bind address |
| `-p, --port` | `18789` | Listen port |
| `--json-logs` | off | Emit JSON-structured logs |

---

## Verification

## Health check

```bash
# CLI
aletheia health

# HTTP
curl -s http://127.0.0.1:18789/api/health | jq
```

Healthy response:

```json
{
  "status": "healthy",
  "version": "...",
  "uptime_seconds": 42,
  "checks": [
    { "name": "session_store", "status": "pass" },
    { "name": "providers", "status": "pass" }
  ]
}
```

Status values: `healthy` (all pass), `degraded` (warnings, e.g. no LLM provider), `unhealthy` (failures).

## Prosoche heartbeat timer

The optional user timer checks the running server and then executes the local
prosoche self-audit task. The timer starts one minute after activation and runs
every five minutes.

```bash
install -m 0755 scripts/aletheia-heartbeat.sh ~/.local/bin/aletheia-heartbeat
mkdir -p ~/.config/systemd/user
cp instance.example/services/aletheia-health.{service,timer} ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now aletheia-health.timer
systemctl --user status aletheia-health.timer
journalctl --user -u aletheia-health --since "30 minutes ago"
```

## System status

```bash
aletheia status
```

Displays agent count, active sessions, uptime, and provider status.

## API smoke test

Create a session and send a message to verify the full request path. Replace `YOUR_TOKEN` with the token from `aletheia credential status`.

```bash
# Create a session (nous_id and session_key are both required)
curl -s -X POST \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "X-Requested-With: aletheia" \
  -H "Content-Type: application/json" \
  -d '{"nous_id": "main", "session_key": "smoke-test"}' \
  http://127.0.0.1:18789/api/v1/sessions | jq .id
```

Use the returned session ID to send a message (the response is an SSE stream):

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "X-Requested-With: aletheia" \
  -H "Content-Type: application/json" \
  -d '{"content": "Hello"}' \
  http://127.0.0.1:18789/api/v1/sessions/SESSION_ID/messages
```

## Prometheus metrics

```bash
curl -s http://127.0.0.1:18789/metrics
```

Exposes `nous_turn_duration_seconds`, `anthropic_requests_total`, `http_requests_total`, and more.

---

## Systemd service (Linux)

A service template lives in `instance.example/services/aletheia.service`.
It uses `%h` (systemd's `$HOME` specifier) so paths resolve automatically, but
it is **not** copy-paste ready: the shipped template contains inline directive
comments and references undefined environment variables (`${ALETHEIA_DATA_DIR}`,
`${ALETHEIA_INSTANCE_DIR}`) that `systemd-analyze verify` will reject. Treat it
as a starting point and customize it before enabling.

```bash
# 1. Copy the template
mkdir -p ~/.config/systemd/user
cp instance.example/services/aletheia.service ~/.config/systemd/user/aletheia.service

# 2. Review and edit the unit file
#    - Move any inline comments to their own lines (systemd does not support
#      inline comments on directive lines).
#    - Replace ${ALETHEIA_DATA_DIR} and ${ALETHEIA_INSTANCE_DIR} with concrete
#      paths, e.g. %h/aletheia/instance/data and %h/aletheia/instance, or
#      define them in the EnvironmentFile loaded by the unit.
#    - Adjust ExecStart if the binary is not at ~/.local/bin/aletheia or the
#      instance is not at ~/aletheia/instance.

# 3. Verify the unit parses
systemd-analyze verify ~/.config/systemd/user/aletheia.service

# 4. Enable and start
systemctl --user daemon-reload
systemctl --user enable --now aletheia

# 5. Persist across logout (run once per user)
loginctl enable-linger
```

The template sets `ExecStart=%h/.local/bin/aletheia -r %h/aletheia/instance`
(`-r` points the binary at the instance root) and loads an optional
`EnvironmentFile` from `%h/aletheia/instance/config/env` (silently ignored if absent).
If your API key is stored in `instance/config/credentials/anthropic.json` (written by
`aletheia init`), no extra environment setup is needed.

View logs:

```bash
journalctl --user -u aletheia -f
```

Verify after start:

```bash
aletheia health
```

---

## Backup

```bash
aletheia backup                     # create backup
aletheia backup --list              # list available backups
aletheia backup --prune --keep 5    # remove old backups
aletheia backup verify <path>       # verify a backup snapshot
```

Backups are stored in `instance/data/backups/`. Always back up before upgrading.

The `--export-json` flag was removed during the SQLite-to-fjall migration
(#3446, #4657). Session archives that are already JSON live under
`instance/data/archive/sessions/`. The cron helper `scripts/backup-cron.sh`
still references the removed flag and should not be installed until it is
updated.

---

## Maintenance

Background maintenance tasks run automatically when the server is running. To check status or trigger manually:

```bash
aletheia maintenance status                     # show all task statuses
aletheia maintenance run trace-rotation         # rotate trace logs
aletheia maintenance run drift-detection        # check instance structure
aletheia maintenance run db-monitor             # check database sizes
aletheia maintenance run all                    # run everything
```

---

## Troubleshooting

See [RUNBOOK.md](RUNBOOK.md) for operational procedures and troubleshooting.

---

## Optional: signal messaging

Aletheia can receive and send messages via [Signal](https://signal.org/) using the [signal-cli](https://github.com/AsamK/signal-cli) JSON-RPC daemon.

1. Install and register signal-cli
2. Start the JSON-RPC daemon: `signal-cli -a +1XXXXXXXXXX daemon --http 8080`
3. Configure in `aletheia.toml`:

```toml
[channels.signal]
enabled = true

[channels.signal.accounts.default]
account = "+1XXXXXXXXXX"
http_host = "localhost"
http_port = 8080
```

4. Add a binding to route messages to an agent:

```toml
[[bindings]]
channel = "signal"
source = "*"
nous_id = "main"
```

---

## Matrix (Phase 3 deployment profile)

Aletheia's public deployment profile preserves homeserver deployment prep for
the Phase 3 Matrix channel implementation against a self-hosted
[conduwuit](https://conduwuit.puppyirl.gay/) homeserver. Aletheia does not
expose a Matrix provider, config surface, or Cargo feature.

### Prerequisites

- A private overlay network or reverse proxy path for hosts that expose conduwuit beyond loopback.
- `podman` 4.4+ with Quadlet generator (`/etc/containers/systemd/`).
- `${CONDUWUIT_DATA_DIR}` writable (the script creates it under `sudo`).

### Deploy the homeserver

```bash
scripts/deploy-conduwuit.sh --server-name matrix.example.com
```

The script:

- pulls a pinned conduwuit container image,
- generates a registration token at `${SECRETS_DIR}/conduwuit-registration-token` (mode `0600`), where `${SECRETS_DIR}` is an operator-managed secrets directory,
- installs a Quadlet unit at `/etc/containers/systemd/conduwuit.container`,
- reloads systemd and starts `conduwuit.service`,
- waits for `http://127.0.0.1:6167/_matrix/client/versions` to return 200.

The service restarts on failure and runs with `NoNewPrivileges`, `ProtectSystem`, and loopback-only publish.

### Register the first user

Use the registration token the script printed (also at `${SECRETS_DIR}/conduwuit-registration-token`). Follow conduwuit's current API docs for the exact endpoint - typically via `element` (web client) against `http://host.example.lan:6167`, selecting "Create account" and pasting the token when prompted.

### Connect an Element client

Point Element (desktop or web) at `http://host.example.lan:6167`. Sign in as the user you registered. Over the private overlay network this URL resolves and authenticates end-to-end.

### Aletheia provider status

No `[channels.matrix]` config surface or `matrix` Cargo feature is exposed
until Phase 3 has real send and receive behavior.
