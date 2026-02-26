# Deployment

Production setup for running Aletheia as a persistent service.

## Quick Setup

```bash
aletheia-setup
```

Installs and starts all services, enables boot persistence, and verifies health. Idempotent — safe to re-run after `aletheia-update`.

**Prerequisites:**
- Runtime built: `infrastructure/runtime/dist/entry.mjs` must exist (run `./setup.sh` or build manually)
- `node` in PATH, `podman` or `docker` for memory services

**What gets installed:**

| Service | Port | Description |
|---------|------|-------------|
| `aletheia` | 18789 | AI gateway |
| `aletheia-memory` | 8230 | Mem0 sidecar (Qdrant + Neo4j) |

Services are enabled on boot via `loginctl enable-linger`.

---

## Manual Control

```bash
systemctl --user status aletheia aletheia-memory
systemctl --user restart aletheia
systemctl --user stop aletheia aletheia-memory
journalctl --user -u aletheia -f
journalctl --user -u aletheia-memory -f
```

## Health Checks

```bash
curl -s http://localhost:18789/health    # Gateway
curl -s http://localhost:8230/health     # Memory sidecar
curl -s http://localhost:6333/healthz    # Qdrant
curl -s http://localhost:7474            # Neo4j
curl -s http://localhost:18789/api/metrics
```

---

## Signal (optional)

```bash
podman compose up -d    # Uses docker-compose.yml in repo root
```

Or native: `signal-cli -u +1XXXXXXXXXX daemon --http --receive-mode=on-start`

Configure via `channels.signal.accounts.default.httpPort` in gateway config.

## Langfuse (optional)

```bash
cd infrastructure/langfuse && podman compose up -d    # Port 3100
```

Set `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` in `~/.aletheia/env`.

## Prosoche (optional)

```bash
cd infrastructure/prosoche
cp config.yaml.example config.yaml && python3 prosoche.py
```

---

## Troubleshooting

**Service won't start:** `journalctl --user -u aletheia -n 50 --no-pager`

**Memory/graph not working:** Check containers are running: `docker ps | grep -E "qdrant|neo4j"`. If stopped: `cd infrastructure/memory && docker compose up -d`.

**Signal not receiving:** Check signal-cli (`curl localhost:8080/v1/about`), verify phone number matches config, check DM policy.

**Config changes:** `systemctl --user restart aletheia`
