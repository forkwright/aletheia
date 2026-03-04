# Deployment

Production operations guide. For first-time setup, see [QUICKSTART.md](QUICKSTART.md).

## System Requirements

- **OS:** Linux or macOS
- **Node.js:** >= 22.12
- **RAM:** 2GB minimum, 4GB+ recommended with memory services
- **Disk:** ~500MB (repo + dependencies), plus data growth
- **Optional:** Docker/Podman (Qdrant, Neo4j), signal-cli, Chromium

## Post-Clone Setup

Gitignored files that need manual creation after a fresh clone:

- `instance/config/aletheia.yaml` - main config (copy from `instance.example/`)
- `instance/config/credentials/` - API keys and secrets
- `infrastructure/memory/sidecar/.venv/` - Python virtual environment for memory sidecar

File permissions:

```bash
git update-index --chmod=+x infrastructure/runtime/aletheia.mjs
```

---

## Boot Persistence

### macOS (launchd)

```bash
aletheia enable    # install plists, start at login
aletheia disable   # unload, remove plists
```

Installs `com.aletheia.gateway` and `com.aletheia.memory` LaunchAgents. Token substitution captures the current `node` path, repo root, and config dir at install time. Re-run `aletheia enable` after Node.js upgrades.

Logs: `instance/logs/gateway.log`, `instance/logs/memory.log`

### Linux (systemd)

```bash
aletheia enable    # install user units, enable for boot
aletheia disable   # disable, remove units
```

Installs `aletheia.service` and `aletheia-memory.service` to `~/.config/systemd/user/`. Requires `loginctl enable-linger` for services to survive logout.

Logs: `journalctl --user -u aletheia -f`

---

## Service Management

```bash
aletheia start     # start services
aletheia stop      # stop gateway
aletheia restart   # restart gateway
```

macOS force restart:

```bash
launchctl kickstart -k gui/$(id -u)/com.aletheia.gateway
```

---

## Health Checks

```bash
aletheia doctor                            # config + connectivity
aletheia status                            # live metrics (requires running gateway)

curl -s http://localhost:18789/health      # gateway
curl -s http://localhost:6333/healthz      # Qdrant
curl -s http://localhost:7474              # Neo4j
curl -s http://localhost:8230/health       # memory sidecar
```

---

## Update / Deploy

```bash
cd <repo-root>
git pull origin main
cd infrastructure/runtime && npx tsdown   # rebuild runtime
cd ../../ui && npm run build              # rebuild UI (if changed)
aletheia restart
```

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| Service won't start | `aletheia doctor` first. Check logs. |
| Port 18789 in use | `fuser -k 18789/tcp && sleep 2 && aletheia start` |
| Wrong node path (macOS) | `aletheia disable && aletheia enable` |
| launchd not loading | `plutil ~/Library/LaunchAgents/com.aletheia.gateway.plist` |
| Memory sidecar unhealthy | Check containers: `podman ps \| grep -E "qdrant\|neo4j"` |
| Signal not receiving | Restart gateway (auto-starts signal-cli) |
| OAuth token expired | Replace in `instance/config/aletheia.yaml`. Router handles 429/5xx failover. |

---

## Optional Integrations

### Signal

```bash
podman compose up -d    # or signal-cli daemon directly
```

Configure `channels.signal.accounts.default` in gateway config.

### Langfuse

```bash
cd infrastructure/langfuse && podman compose up -d    # Port 3100
```

Set `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` in `instance/config/env`.

### Prosoche

```bash
cd infrastructure/prosoche
cp config.yaml.example config.yaml && python3 prosoche.py
```

