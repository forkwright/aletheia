# Deployment

Production setup for running Aletheia as a persistent service.

## Gateway Service

Install as a user systemd service (no root required):

```bash
mkdir -p ~/.config/systemd/user
cp config/services/aletheia.service ~/.config/systemd/user/aletheia.service
systemctl --user daemon-reload
systemctl --user enable --now aletheia
```

The service file uses `%h` (home directory) specifiers. Create the env file it expects:

```bash
mkdir -p ~/.aletheia
echo "ALETHEIA_ROOT=/path/to/aletheia" > ~/.aletheia/env
```

Alternatively, use `aletheia start` for process-managed startup without systemd.

## Signal

### Container (recommended)

```bash
podman compose up -d    # Uses docker-compose.yml in repo root
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

To run as a user service, create `~/.config/systemd/user/aletheia-memory.service`:

```ini
[Unit]
Description=Aletheia Memory Sidecar
After=network-online.target

[Service]
Type=simple
WorkingDirectory=%h/aletheia/infrastructure/memory/sidecar
EnvironmentFile=%h/.aletheia/env
ExecStart=%h/aletheia/infrastructure/memory/sidecar/.venv/bin/uvicorn aletheia_memory.app:app --host 127.0.0.1 --port 8230
Restart=on-failure

[Install]
WantedBy=default.target
```

Dependencies: `cd infrastructure/memory && podman compose up -d` (Qdrant + Neo4j).

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
cd infrastructure/langfuse && podman compose up -d    # Port 3100
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

**Service won't start:** `journalctl --user -u aletheia -n 50 --no-pager`

**Signal not receiving:** Check signal-cli (`curl localhost:8080/v1/about`), verify phone number matches config, check DM policy.

**Memory extraction failing:** Check sidecar logs (`journalctl --user -u aletheia-memory -f`), verify Qdrant/Neo4j running.

**Config changes:** `systemctl --user restart aletheia` or `aletheia restart`.
