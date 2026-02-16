# Deployment

Production setup for running Aletheia as a persistent service.

## Service Account

Create a dedicated service account:

```bash
sudo useradd -r -m -s /bin/bash aletheia
sudo -u aletheia mkdir -p ~/.aletheia/credentials
```

## Systemd Service

Create `/etc/systemd/system/aletheia.service`:

```ini
[Unit]
Description=Aletheia Gateway
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=aletheia
Group=aletheia
WorkingDirectory=/path/to/aletheia
EnvironmentFile=/path/to/aletheia/shared/config/aletheia.env
ExecStart=/usr/bin/node /path/to/aletheia/infrastructure/runtime/aletheia.mjs gateway
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now aletheia
```

## Signal Integration

### Container (recommended)

```bash
docker compose up -d    # Uses docker-compose.yml in repo root
```

### Native Install

Install signal-cli and run in JSON-RPC mode:

```bash
signal-cli -u +1XXXXXXXXXX daemon --http --receive-mode=on-start
```

Configure the gateway to connect via `channels.signal.accounts.default.httpPort` (default: 8080).

## Memory Sidecar

The Mem0 sidecar handles automatic memory extraction and retrieval.

### Setup

```bash
cd infrastructure/memory/sidecar
uv venv && source .venv/bin/activate
uv pip install -e .
```

### Systemd Service

Create `/etc/systemd/system/aletheia-memory.service`:

```ini
[Unit]
Description=Aletheia Memory Sidecar
After=network-online.target docker.service

[Service]
Type=simple
User=aletheia
WorkingDirectory=/path/to/aletheia/infrastructure/memory/sidecar
EnvironmentFile=/path/to/aletheia/shared/config/aletheia.env
ExecStart=/path/to/aletheia/infrastructure/memory/sidecar/.venv/bin/uvicorn aletheia_memory.app:app --host 127.0.0.1 --port 8230
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### Dependencies

```bash
cd infrastructure/memory
docker compose up -d    # Qdrant (:6333) + Neo4j (:7474/:7687)
```

### Memory Plugin

Enable in gateway config:

```json
{
  "plugins": {
    "enabled": true,
    "load": {
      "paths": ["infrastructure/memory/aletheia-memory"]
    }
  }
}
```

The plugin hooks into agent lifecycle:
- `before_agent_start` — recalls relevant memories into context
- `agent_end` — extracts new memories from the conversation

## Langfuse (Observability)

Optional session tracing and metrics.

```bash
cd infrastructure/langfuse
docker compose up -d    # Langfuse on :3100
```

Configure API keys via the Langfuse dashboard. Set `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` in `aletheia.env`.

## Web UI

Build and deploy the Svelte 5 chat interface:

```bash
cd ui
npm install
npm run build    # Outputs to ui/dist/
```

The gateway serves `ui/dist/` as static files at `/ui` with SPA fallback. Hashed assets get immutable cache headers. `index.html` gets `no-cache` for instant updates on rebuild.

If `ui/dist/` doesn't exist, the gateway falls back to a minimal inline status dashboard.

## Prosoche (Adaptive Attention)

Optional daemon that generates directed awareness signals for agents.

```bash
cd infrastructure/prosoche
cp config.yaml.example config.yaml    # Configure signals and weights
python3 prosoche.py
```

Can also run as a systemd service.

## Services Summary

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | Yes |
| aletheia-memory | 8230 | Recommended |
| qdrant | 6333 | If using Mem0 |
| neo4j | 7474/7687 | If using Mem0 |
| langfuse | 3100 | Optional |

## Health Checks

```bash
# Gateway
curl -s http://localhost:18789/health

# Memory sidecar
curl -s http://localhost:8230/health

# Qdrant
curl -s http://localhost:6333/healthz

# Neo4j
curl -s http://localhost:7474

# Full metrics
curl -s http://localhost:18789/api/metrics
```

## Troubleshooting

### Service won't start
```bash
journalctl -u aletheia -n 50 --no-pager
```

### Signal not receiving messages
- Verify signal-cli is running: `curl -s http://localhost:8080/v1/about`
- Check the registered number matches config
- Verify DM policy allows the sender

### Memory extraction failing
- Check sidecar logs: `journalctl -u aletheia-memory -f`
- Verify Qdrant and Neo4j are running
- Test sidecar directly: `curl -s http://localhost:8230/health`

### Config changes not taking effect
Config requires a service restart:
```bash
sudo systemctl restart aletheia
```
