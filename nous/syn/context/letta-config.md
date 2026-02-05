# Letta Configuration

## Server
- **URL:** http://localhost:8283
- **Docs:** http://localhost:8283/docs
- **Status:** Running via Docker

## Agent: syn-memory
- **ID:** agent-644c65f8-72d4-440d-ba5d-330a3d391f8e
- **Model:** anthropic/claude-3-5-haiku
- **Created:** 2026-01-28

## Memory Blocks
| Label | Purpose |
|-------|---------|
| persona | Who I am |
| user | Who Cody is |
| preferences | Communication style |
| context | Current projects |

## Docker
```bash
cd /tmp/letta && docker compose up -d letta_db letta_server
```

## API Examples
```bash
# Send message
curl -X POST "http://localhost:8283/v1/agents/agent-644c65f8-72d4-440d-ba5d-330a3d391f8e/messages" \
  -H "Content-Type: application/json" \
  -d '{"messages": [{"role": "user", "content": "..."}]}'

# Get memory blocks
curl http://localhost:8283/v1/agents/agent-644c65f8-72d4-440d-ba5d-330a3d391f8e/memory/
```

## Next Steps
- [ ] Build Clawdbot integration (sync memories)
- [ ] Configure archival memory for long-term facts
- [ ] Set up sleeptime agent for background consolidation

---
*Created: 2026-01-28*
