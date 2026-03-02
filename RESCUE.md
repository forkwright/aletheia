# Recovery Guide

Full restore from scratch.

## Quick Reference

| What | Where |
|------|-------|
| Service status | `aletheia status` |
| Config | `~/.aletheia/aletheia.json` |
| Runtime source | `infrastructure/runtime/` |
| Sessions DB | `~/.aletheia/sessions.db` |
| Memory sidecar | `curl http://localhost:8230/health` |

## Full Recovery

```bash
# 1. Clone and build
git clone https://github.com/CKickertz/ergon.git && cd ergon
./setup.sh

# 2. Memory infrastructure (optional)
cd infrastructure/memory && podman compose up -d

# 3. Config
mkdir -p ~/.aletheia/credentials
cp config/aletheia.example.json ~/.aletheia/aletheia.json
# Edit: agents, credentials, branding

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

## Troubleshooting

See [DEPLOYMENT.md](docs/DEPLOYMENT.md#troubleshooting).
