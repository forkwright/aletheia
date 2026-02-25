# Rescue Document

Recovery guide for restoring Aletheia from scratch.

## Quick Reference

| What | Where |
|------|-------|
| Service | `aletheia status` |
| Config | `~/.aletheia/aletheia.json` |
| Runtime | `infrastructure/runtime/` |
| Sessions DB | `~/.aletheia/sessions.db` |
| Memory sidecar | `curl http://localhost:8230/health` (port 8230) |

## Full Recovery

```bash
# 1. Clone and build
git clone https://github.com/forkwright/aletheia.git && cd aletheia
./setup.sh    # builds runtime + UI, installs CLI, starts gateway

# 2. Memory infrastructure (optional)
cd infrastructure/memory && podman compose up -d  # Qdrant + Neo4j
# Start Mem0 sidecar separately — see docs/DEPLOYMENT.md#memory-sidecar

# 3. Gateway config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit: agents, credentials

# 4. Start
aletheia start

# 5. Verify
aletheia status
curl -s http://localhost:18789/health
```

## Post-Clone Checklist

Gitignored files that need manual setup:

- `shared/config/aletheia.env`
- `~/.aletheia/aletheia.json`
- `~/.aletheia/credentials/`
- `infrastructure/memory/sidecar/.venv/`

## File Permissions

```bash
git update-index --chmod=+x infrastructure/runtime/aletheia.mjs
setfacl -m u:<service-user>:rwx shared/bin/*
```

For troubleshooting, see [DEPLOYMENT.md](docs/DEPLOYMENT.md#troubleshooting).
