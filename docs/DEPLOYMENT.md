# Deployment

> This file covers the full production deployment reference. For focused sub-topics see:
> - [QUICKSTART.md](QUICKSTART.md): minimal first-time setup
> - [CONFIGURATION.md](CONFIGURATION.md): full configuration reference
> - [troubleshooting.md](troubleshooting.md): common issues and solutions

Step-by-step guide from bare Linux or macOS to a running Aletheia instance. For configuration details, see [CONFIGURATION.md](CONFIGURATION.md). For upgrading an existing installation, see [UPGRADING.md](UPGRADING.md).

---

## Getting started (first-time setup)

Three commands from zero to a running instance:

```bash
# 1. Install the binary (or build from source: cargo build --release)
curl -L https://github.com/forkwright/aletheia/releases/latest/download/aletheia-linux-x86_64 -o aletheia
chmod +x aletheia && sudo mv aletheia /usr/local/bin/

# 2. Initialize (creates instance directory, config, credentials, first agent)
aletheia init

# 3. Start
aletheia
```

The init wizard prompts for your API key, model, agent name, and bind address, then writes a complete instance. For non-interactive (CI/scripting) use:

```bash
ANTHROPIC_API_KEY=sk-ant-... aletheia init --yes --instance-root /srv/aletheia/instance
```

Verify with `aletheia health`. A `healthy` response means the server is running with a registered LLM provider. If the status is `degraded`, verify your API key is set and the config loaded it.

> **Manual setup:** If you prefer to configure everything by hand instead of using the wizard, see [Manual: copy the example scaffold](#manual-copy-the-example-scaffold) below.

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

### Prebuilt binary

Download from [GitHub Releases](https://github.com/forkwright/aletheia/releases):

```bash
# Download binary and checksum (example for Linux x86_64)
gh release download latest -p 'aletheia-linux-amd64*'

# Verify
sha256sum -c aletheia-linux-amd64.sha256

# Install
chmod +x aletheia-linux-amd64
sudo mv aletheia-linux-amd64 /usr/local/bin/aletheia
```

### Build from source

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
cargo build --release
```

Binary: `target/release/aletheia`

### Headless build (no TUI)

The TUI (terminal dashboard) is compiled in by default. To build without it (useful for servers, containers, or minimal footprints), disable the `tui` feature:

```bash
cargo build --release --no-default-features --features tls
```

The headless binary accepts all the same CLI flags and API endpoints. The `aletheia status` command falls back to a plain-text summary when TUI is absent.

### Shell completions

See [SHELL-COMPLETIONS.md](SHELL-COMPLETIONS.md) for setting up tab-completion in bash, zsh, and fish.

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

Then configure `instance/config/aletheia.toml` (see the Configuration section). The scaffold provides a template with all required directories and example configuration files.

This creates the full directory scaffold:

```text
instance/
├── config/
│   ├── aletheia.toml       # Main config
│   ├── credentials/        # API keys, secrets
│   └── tls/                # TLS certs (optional)
├── data/                   # SQLite databases, backups
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

The init wizard writes a complete `config/aletheia.toml`. If you are setting up manually, create one from `instance.example/config/aletheia.toml.example`:

```bash
# If you didn't use the init wizard
cp instance/config/aletheia.toml.example instance/config/aletheia.toml
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

The workspace path can be relative (relative to the instance root) or absolute. In this example, `instance/nous/main` is relative and will resolve to `./instance/instance/nous/main` when the instance root is `./instance`. For absolute paths, use the full filesystem path: `/srv/aletheia/instance/nous/main`.

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

CSRF protection is **enabled by default**. All state-changing requests (POST, PUT, DELETE, PATCH) to `/api/v1/` must include the header:

```
X-Requested-With: aletheia
```

Include this header on every mutating curl call:

```bash
curl -X POST \
  -H "Authorization: Bearer your-token" \
  -H "X-Requested-With: aletheia" \
  -H "Content-Type: application/json" \
  -d '{"nous_id": "main", "session_key": "default"}' \
  http://127.0.0.1:18789/api/v1/sessions
```

Missing the CSRF header returns `403 Forbidden`. No config change is needed to enable this; it is on by default.

#### Disable CSRF for development

To turn off CSRF checks in a local development instance, add to `aletheia.toml`:

```toml
[gateway.csrf]
enabled = false
```

With CSRF disabled, the `X-Requested-With` header is no longer required. Do not disable CSRF on any instance exposed to a network.

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

> **Manual alternative:** Steps 1–3 above apply to both first-time manual setups and adding agents to an existing instance. If you used the init wizard, only step 3 may be needed to add additional agents.

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

The startup sequence:
1. Discovers instance root (oikos)
2. Loads config cascade (defaults + YAML + env vars)
3. Opens session store (SQLite, auto-creates `data/sessions.db`)
4. Registers LLM provider (Anthropic, if API key is set)
5. Registers built-in tools
6. Spawns nous actors (one per configured agent)
7. Starts daemon (background maintenance tasks)
8. Starts channel listeners (Signal, if configured)
9. Starts HTTP gateway on configured bind:port

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

### Health check

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

### System status

```bash
aletheia status
```

Displays agent count, active sessions, uptime, and provider status.

### API smoke test

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

### Prometheus metrics

```bash
curl -s http://127.0.0.1:18789/metrics
```

Exposes `nous_turn_duration_seconds`, `anthropic_requests_total`, `http_requests_total`, and more.

---

## Systemd service (Linux)

A ready-to-use service template lives in `instance.example/services/aletheia.service`.
It uses `%h` (systemd's `$HOME` specifier) so paths resolve automatically.

```bash
# 1. Copy the template
mkdir -p ~/.config/systemd/user
cp instance.example/services/aletheia.service ~/.config/systemd/user/aletheia.service

# 2. Adjust paths if needed (binary not at ~/.local/bin/, instance not at ~/aletheia/instance/)
#    Edit ~/.config/systemd/user/aletheia.service and update ExecStart and ALETHEIA_ROOT.

# 3. Enable and start
systemctl --user daemon-reload
systemctl --user enable --now aletheia

# 4. Persist across logout (run once per user)
loginctl enable-linger
```

The template sets `ALETHEIA_ROOT=%h/aletheia/instance` and loads an optional
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
aletheia backup --export-json       # export sessions as JSON
```

Backups are stored in `instance/data/backups/`. Always back up before upgrading.

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

See [troubleshooting.md](troubleshooting.md) for common issues and fixes.

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
