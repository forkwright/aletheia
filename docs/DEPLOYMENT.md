# Deployment

Step-by-step guide from bare Linux or macOS to a running Aletheia instance. For configuration details, see [CONFIGURATION.md](CONFIGURATION.md). For upgrading an existing installation, see [UPGRADING.md](UPGRADING.md).

---

## System Requirements

### Hardware

| | Minimum | Recommended |
|---|---|---|
| CPU | 2 cores | 4 cores |
| RAM | 2 GB | 4 GB (candle model loading) |
| Disk | 1 GB | 10 GB (session data growth) |

### Operating System

| Target | Notes |
|--------|-------|
| `x86_64-unknown-linux-gnu` | glibc-based Linux |
| `aarch64-unknown-linux-gnu` | ARM64 Linux (Raspberry Pi 4+, AWS Graviton) |
| `x86_64-apple-darwin` | macOS 12+ Intel |
| `aarch64-apple-darwin` | macOS 12+ Apple Silicon |

### Software

- Links dynamically against glibc only (Linux). No other runtime dependencies.
- **Build from source:** Rust 1.85+ (edition 2024), Cargo
- **Optional:** signal-cli for Signal messaging channel

### Network

- **Outbound:** HTTPS to your LLM provider (default: `api.anthropic.com:443`)
- **Inbound:** configurable listen port (default `18789`) for API
- **Local:** signal-cli JSON-RPC if Signal channel is enabled (default `localhost:8080`)

See [NETWORK.md](NETWORK.md) for the complete network call inventory.

---

## Installation

### Prebuilt Binary

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

### Build from Source

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
cargo build --release
```

Binary: `target/release/aletheia`

### Headless Build (no TUI)

The TUI (terminal dashboard) is compiled in by default. To build without it — useful for servers, containers, or minimal footprints — disable the `tui` feature:

```bash
cargo build --release --no-default-features --features tls
```

The headless binary accepts all the same CLI flags and API endpoints. The `aletheia status` command falls back to a plain-text summary when TUI is absent.

### Shell Completions

See [SHELL-COMPLETIONS.md](SHELL-COMPLETIONS.md) for setting up tab-completion in bash, zsh, and fish.

---

## Instance Setup

The instance directory holds all runtime state: config, databases, agent workspaces, logs. It is gitignored — the platform code ships without instance data.

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

```bash
cp -r instance.example instance
```

Then configure `instance/config/aletheia.toml` (see below).

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

### Instance Discovery

The binary finds the instance directory in this order:

1. `--instance-root` CLI flag (explicit path)
2. `ALETHEIA_ROOT` environment variable
3. `./instance` relative to the working directory

---

## Configuration

The init wizard writes a complete `config/aletheia.toml`. If you are setting up manually, create one:

```yaml
gateway:
  port: 18789
  bind: localhost

agents:
  defaults:
    model:
      primary: claude-sonnet-4-6
  list:
    - id: main
      name: Main
      default: true
      workspace: instance/nous/main
```

The config cascade loads in order (later wins): compiled defaults, YAML file, `ALETHEIA_` environment variables. See [CONFIGURATION.md](CONFIGURATION.md) for the complete reference.

---

## Credentials

### LLM Provider

Set the Anthropic API key as an environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

The binary reads `ANTHROPIC_API_KEY` from the environment at startup. If unset, it starts without an LLM provider (health check reports degraded status).

### TLS (Optional)

Generate self-signed certificates for LAN use:

```bash
aletheia tls generate --output-dir instance/config/tls --days 365 --san localhost --san 192.168.1.100
```

Then enable in config:

```yaml
gateway:
  tls:
    enabled: true
    cert_path: config/tls/cert.pem
    key_path: config/tls/key.pem
```

---

## Agent Setup

Each agent needs a workspace directory under `instance/nous/`:

```bash
cp -r instance/nous/_template instance/nous/main
```

Edit the bootstrap files:
- `SOUL.md` — agent identity and character
- `IDENTITY.md` — display name, emoji
- `GOALS.md` — current goals
- `MEMORY.md` — persistent operational memory

Register the agent in `aletheia.toml`:

```yaml
agents:
  list:
    - id: main
      default: true
```

---

## First Run

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

### CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `-r, --instance-root` | auto-discover | Path to instance directory |
| `-l, --log-level` | `info` | Log level (trace, debug, info, warn, error) |
| `--bind` | `localhost` | Bind address |
| `-p, --port` | `18789` | Listen port |
| `--json-logs` | off | Emit JSON-structured logs |

---

## Verification

### Health Check

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
  "version": "0.10.0",
  "uptime_seconds": 42,
  "checks": [
    { "name": "session_store", "status": "pass" },
    { "name": "providers", "status": "pass" }
  ]
}
```

Status values: `healthy` (all pass), `degraded` (warnings, e.g. no LLM provider), `unhealthy` (failures).

### System Status

```bash
aletheia status
```

Displays agent count, active sessions, uptime, and provider status.

### Prometheus Metrics

```bash
curl -s http://127.0.0.1:18789/metrics
```

Exposes `nous_turn_duration_seconds`, `anthropic_requests_total`, `http_requests_total`, and more.

---

## Systemd Service (Linux)

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

| Problem | Fix |
|---------|-----|
| `ANTHROPIC_API_KEY not set` | Export the env var or add to systemd `Environment=` |
| Port already in use | `fuser -k 18789/tcp` then restart, or change `gateway.port` in config |
| Config parse error | Check YAML syntax, verify field names match [CONFIGURATION.md](CONFIGURATION.md) |
| Health returns `degraded` | No LLM provider registered — check API key |
| Health returns `unhealthy` | Session store failed to open — check `instance/data/` permissions |
| Signal not receiving | Verify signal-cli daemon is running on configured host:port |
| Bind address error | Check `--bind` flag or `gateway.bind` config — `lan` resolves to LAN interface |

---

## Optional: Signal Messaging

Aletheia can receive and send messages via [Signal](https://signal.org/) using the [signal-cli](https://github.com/AsamK/signal-cli) JSON-RPC daemon.

1. Install and register signal-cli
2. Start the JSON-RPC daemon: `signal-cli -a +1XXXXXXXXXX daemon --http 8080`
3. Configure in `aletheia.toml`:

```yaml
channels:
  signal:
    enabled: true
    accounts:
      default:
        account: "+1XXXXXXXXXX"
        http_host: localhost
        http_port: 8080
```

4. Add a binding to route messages to an agent:

```yaml
bindings:
  - channel: signal
    source: "*"
    nous_id: main
```
