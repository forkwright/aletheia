# Rescue Document

Recovery guide for restoring Aletheia from scratch. For architecture and full docs, see `README.md`.

## Quick Reference

| What | Where |
|------|-------|
| Service | `systemctl status aletheia` |
| Config | `~/.aletheia/aletheia.json` |
| Runtime | `infrastructure/runtime/` (tsdown, ~400KB) |
| Web UI | `/ui` (Svelte 5, built from `ui/`) |
| Sessions DB | `~/.aletheia/sessions.db` |
| Memory sidecar | `systemctl status aletheia-memory` (port 8230) |

## Recovery Steps

### Full Recovery (from scratch)

```bash
# 1. Clone
git clone https://github.com/forkwright/aletheia.git
cd aletheia

# 2. Environment
cp .env.example shared/config/aletheia.env
# Edit aletheia.env: fill in API keys and paths
# Must be systemd EnvironmentFile compatible (no `export`, no variable refs)

# 3. Build runtime
cd infrastructure/runtime && npm install && npx tsdown && cd ../..

# 4. Build web UI
cd ui && npm install && npm run build && cd ..

# 5. Memory infrastructure
cd infrastructure/memory && docker compose up -d  # Qdrant + Neo4j
cd sidecar && uv venv && source .venv/bin/activate && uv pip install -e .
sudo cp aletheia-memory.service /etc/systemd/system/
sudo systemctl enable --now aletheia-memory

# 6. Gateway config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit: agents, bindings, Signal number

# 7. First agent
cp -r nous/_example nous/your-agent
# Edit SOUL.md, USER.md, IDENTITY.md

# 8. Start
sudo cp aletheia.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now aletheia

# 9. Verify
curl -s http://localhost:18789/health
curl -s http://localhost:8230/health
curl -s http://localhost:6333/healthz
# Web UI: http://localhost:18789/ui
```

### Post-Clone Checklist

These are gitignored and need manual setup:

- `shared/config/aletheia.env` - environment variables
- `~/.aletheia/aletheia.json` - gateway config
- `~/.aletheia/credentials/` - API keys
- `infrastructure/memory/sidecar/.venv/` - Python venv

### File Permissions

- `aletheia.mjs` must be executable: `git update-index --chmod=+x infrastructure/runtime/aletheia.mjs`
- Service user needs ACL on shared/bin: `setfacl -m u:<user>:rwx shared/bin/*`

### Regenerate Compiled Files

```bash
compile-context          # All AGENTS.md + PROSOCHE.md
generate-tools-md        # All TOOLS.md
```

## Troubleshooting

### Service won't start
```bash
journalctl -u aletheia -n 50 --no-pager
```

### Signal not receiving
- signal-cli running? `curl -s http://localhost:8080/v1/about`
- Phone number matches config?
- DM policy allows the sender?

### Memory extraction failing
- Sidecar: `systemctl status aletheia-memory`
- Qdrant: `curl -s http://localhost:6333/healthz`
- Neo4j: `curl -s http://localhost:7474`
- Direct test: `curl -s -X POST http://localhost:8230/search -H 'Content-Type: application/json' -d '{"query":"test","user_id":"default","limit":5}'`

### Config changes not taking effect
Requires restart: `sudo systemctl restart aletheia`

## Services

| Service | Port | Required |
|---------|------|----------|
| aletheia | 18789 | Yes |
| signal-cli | 8080 | Yes |
| aletheia-memory | 8230 | Recommended |
| qdrant | 6333 | If using Mem0 |
| neo4j | 7474/7687 | If using Mem0 |
| langfuse | 3100 | Optional |
