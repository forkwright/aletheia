# Memory Systems Research

*Via Perplexity, 2026-01-28*

## Recommendation: Letta (formerly MemGPT)

**Why Letta:**
- Self-hosted option (Docker)
- Built-in memory hierarchy
- Model-agnostic (works with Claude)
- REST API for integration
- Background "sleeptime" agent for memory consolidation

## Architecture

```
┌─────────────────────────────────────────────┐
│                 Core Memory                  │
│  (always in context, agent-editable)        │
│  ├── persona      # who I am                │
│  ├── user_profile # who Cody is             │
│  ├── routines     # how I work              │
│  └── projects     # current focus           │
├─────────────────────────────────────────────┤
│               Recall Memory                  │
│  (recent messages, searchable)              │
├─────────────────────────────────────────────┤
│              Archival Memory                 │
│  (long-term facts, vector embeddings)       │
│  - decisions, preferences, history          │
│  - indexed documents                        │
└─────────────────────────────────────────────┘
```

## Key Features

1. **Memory blocks** — structured data that lives in context
2. **Self-editing** — agent can update its own memory
3. **Conversation search** — full message history searchable
4. **Archival storage** — vector DB for long-term facts
5. **Sleeptime agent** — background consolidation/cleanup

## Integration with Clawdbot

Options:
1. **Standalone Letta server** — run separately, Clawdbot calls API
2. **Letta as memory backend** — replace current file-based memory
3. **Hybrid** — keep files for human readability, sync to Letta

### API Flow
```
client → Clawdbot → agents.messages.create() → Letta
                                               ↓
                         reads memory blocks, searches archival
                                               ↓
                         updates memory via tools, responds
                                               ↓
                         state persisted in Letta DB
```

## Self-Hosting

```bash
# Docker compose
git clone https://github.com/letta-ai/letta
cd letta
docker compose up -d
```

- Postgres for state
- Embedding model (local or API)
- Compatible with Claude API

## Resources

- https://docs.letta.com
- https://github.com/letta-ai/letta
- https://docs.letta.com/guides/selfhosting

## Next Steps

1. Deploy Letta on worker-node (Docker)
2. Configure Claude as LLM backend
3. Design memory block schema for our use case
4. Build Clawdbot integration (MCP or direct API)
5. Migrate current MEMORY.md content

---

*Research complete. Ready to prototype.*
