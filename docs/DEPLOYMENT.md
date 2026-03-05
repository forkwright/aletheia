# Deployment

Step-by-step guide from bare Linux or macOS to a running Aletheia instance. For configuration details, see [CONFIGURATION.md](CONFIGURATION.md). For upgrading an existing installation, see [UPGRADING.md](UPGRADING.md).

---

## System Requirements

### Hardware

| | Minimum | Recommended |
|---|---|---|
| CPU | 2 cores | 4 cores |
| RAM | 2 GB | 4 GB (fastembed model loading) |
| Disk | 1 GB | 10 GB (session data growth) |

### Operating System

| Target | Notes |
|--------|-------|
| `x86_64-unknown-linux-gnu` | glibc-based Linux |
| `aarch64-unknown-linux-gnu` | ARM64 Linux (Raspberry Pi 4+, AWS Graviton) |
| `x86_64-apple-darwin` | macOS 12+ Intel |
| `aarch64-apple-darwin` | macOS 12+ Apple Silicon |

### Software

- No runtime dependencies beyond glibc (Linux). The binary is statically linked.
- **Build from source:** Rust 1.85+ (edition 2024), Cargo
- **Optional:** signal-cli for Signal messaging channel

### Network

- **Outbound:** HTTPS to your LLM provider (default: `api.anthropic.com:443`)
- **Inbound:** configurable listen port (default `18789`) for API and web UI
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

---

## Instance Setup

The instance directory holds all runtime state: config, databases, agent workspaces, logs. It is gitignored — the platform code ships without instance data.

```bash
cp -r instance.example instance
```

This creates the full directory scaffold:

```text
instance/
├── config/
│   ├── aletheia.yaml       # Main config (from aletheia.yaml.example)
│   ├── credentials/        # API keys, secrets
│   └── tls/                # TLS certs (optional)
├── data/                   # SQLite databases, backups
├── logs/traces/            # Trace files
├── nous/                   # Agent workspaces
│   └── _template/          # Template for new agents
├── shared/                 # Shared tools, coordination, hooks
├── theke/                  # Human + agent collaborative space
├── signal/                 # signal-cli data (if using Signal)
└── ui/                     # Web UI build artifacts
```

See [instance.example/README.md](../instance.example/README.md) for the three-tier cascade and what goes where.

### Instance Discovery

The binary finds the instance directory in this order:

1. `--instance-root` CLI flag (explicit path)
2. `ALETHEIA_ROOT` environment variable
3. `./instance` relative to the working directory

---

## Configuration

Copy and edit the example config:

```bash
cp instance/config/aletheia.yaml.example instance/config/aletheia.yaml
```

Minimal working config:

```yaml
gateway:
  port: 18789
  bind: lan

agents:
  list:
    - id: main
      default: true
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

Register the agent in `aletheia.yaml`:

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
| `--bind` | `127.0.0.1` | Bind address |
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

Create a user service unit:

```bash
mkdir -p ~/.config/systemd/user
```

```ini
# ~/.config/systemd/user/aletheia.service
[Unit]
Description=Aletheia cognitive agent runtime
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Environment=ALETHEIA_ROOT=/srv/aletheia/instance
Environment=ANTHROPIC_API_KEY=sk-ant-...
ExecStart=/usr/local/bin/aletheia
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
```

Enable and start:

```bash
systemctl --user daemon-reload
systemctl --user enable --now aletheia
loginctl enable-linger    # services survive logout
```

View logs:

```bash
journalctl --user -u aletheia -f
```

> **Note:** The file at `config/services/aletheia.service` in the repo still references the TypeScript runtime. Use the unit file above for the Rust binary.

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
3. Configure in `aletheia.yaml`:

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
