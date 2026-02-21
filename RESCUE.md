# Rescue Document

Recovery guide for restoring Aletheia from scratch.

## Quick Reference

| What | Where |
|------|-------|
| Service | `systemctl status aletheia` |
| Config | `~/.aletheia/aletheia.json` |
| Runtime | `infrastructure/runtime/` |
| Sessions DB | `~/.aletheia/sessions.db` |
| Memory sidecar | `systemctl status aletheia-memory` (port 8230) |

## Full Recovery

```bash
# 1. Clone and build
git clone https://github.com/forkwright/aletheia.git && cd aletheia
cd infrastructure/runtime && npm install && npx tsdown && cd ../..
cd ui && npm install && npm run build && cd ..

# 2. Environment
cp .env.example shared/config/aletheia.env
# Edit: ANTHROPIC_API_KEY, ALETHEIA_ROOT

# 3. Memory infrastructure
cd infrastructure/memory && docker compose up -d  # Qdrant + Neo4j
cd sidecar && uv venv && source .venv/bin/activate && uv pip install -e .
sudo cp aletheia-memory.service /etc/systemd/system/
sudo systemctl enable --now aletheia-memory

# 4. Gateway config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit: agents, bindings, Signal number

# 5. First agent
cp -r nous/_example nous/your-agent
# Edit SOUL.md, USER.md, IDENTITY.md

# 6. Start
sudo cp aletheia.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now aletheia

# 7. Verify
curl -s http://localhost:18789/health
curl -s http://localhost:8230/health
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

## Regenerate Compiled Files

```bash
compile-context          # All AGENTS.md + PROSOCHE.md
generate-tools-md        # All TOOLS.md
```

For troubleshooting, see [DEPLOYMENT.md](docs/DEPLOYMENT.md#troubleshooting).
