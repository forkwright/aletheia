# Deployment

Production setup for running Aletheia as a persistent service.

## Service Account

```bash
sudo useradd -r -m -s /bin/bash aletheia
sudo -u aletheia mkdir -p ~/.aletheia/credentials
```

## Gateway Service

`/etc/systemd/system/aletheia.service`:

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
sudo systemctl daemon-reload && sudo systemctl enable --now aletheia
```

## Signal

### Container (recommended)

```bash
docker compose up -d    # Uses docker-compose.yml in repo root
```

### Native

```bash
signal-cli -u +1XXXXXXXXXX daemon --http --receive-mode=on-start
```

Configure via `channels.signal.accounts.default.httpPort` in gateway config.

## Memory Sidecar

```bash
cd infrastructure/memory/sidecar
uv venv && source .venv/bin/activate && uv pip install -e .
```

`/etc/systemd/system/aletheia-memory.service`:

```ini
[Unit]
Description=Aletheia Memory Sidecar
After=network-online.target docker.service

[Service]
Type=simple
User=aletheia
WorkingDirectory=/path/to/aletheia/infrastructure/memory/sidecar
EnvironmentFile=/path/to/aletheia/shared/config/aletheia.env
ExecStart=/path/to/sidecar/.venv/bin/uvicorn aletheia_memory.app:app --host 127.0.0.1 --port 8230
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Dependencies: `cd infrastructure/memory && docker compose up -d` (Qdrant + Neo4j).

Enable the memory plugin in gateway config:

```json
{
  "plugins": {
    "enabled": true,
    "load": { "paths": ["infrastructure/memory/aletheia-memory"] }
  }
}
```

## Web UI

```bash
cd ui && npm install && npm run build
```

Served at `/ui` by the gateway. Hashed assets get immutable cache headers.

## Langfuse (optional)

```bash
cd infrastructure/langfuse && docker compose up -d    # Port 3100
```

Set `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` in `aletheia.env`.

## Prosoche (optional)

Adaptive attention daemon:

```bash
cd infrastructure/prosoche
cp config.yaml.example config.yaml && python3 prosoche.py
```

## Health Checks

```bash
curl -s http://localhost:18789/health     # Gateway
curl -s http://localhost:8230/health      # Memory sidecar
curl -s http://localhost:6333/healthz     # Qdrant
curl -s http://localhost:7474             # Neo4j
curl -s http://localhost:18789/api/metrics  # Full metrics
```

## Troubleshooting

**Service won't start:** `journalctl -u aletheia -n 50 --no-pager`

**Signal not receiving:** Check signal-cli (`curl localhost:8080/v1/about`), verify phone number matches config, check DM policy.

**Memory extraction failing:** Check sidecar (`journalctl -u aletheia-memory -f`), verify Qdrant/Neo4j running.

**Config changes:** Require `sudo systemctl restart aletheia`.
