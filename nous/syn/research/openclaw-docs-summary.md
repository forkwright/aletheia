# OpenClaw Official Documentation Summary
**Source:** https://docs.openclaw.ai
**Fetched:** 2026-02-03

## Key Documentation Pages

| Page | URL | What it covers |
|------|-----|----------------|
| Index | /start/hubs | Full sitemap of all docs |
| Memory | /concepts/memory | Memory architecture, vector search, QMD |
| Model Failover | /concepts/model-failover | Auth rotation, cooldowns, fallbacks |
| Multi-Agent | /concepts/multi-agent | Routing, bindings, per-agent config |
| Memory Research | /experiments/research/memory | Hindsight/Letta-style memory design |
| Skills | /tools/skills | Skill format, gating, ClawHub |

## Memory Architecture (Built-in)

### File-Based Memory
```
~/.openclaw/workspace/
  MEMORY.md              # Curated long-term (only in main session)
  memory/
    YYYY-MM-DD.md        # Daily logs (read today + yesterday)
```

### Vector Memory Search
- **Providers:** local (node-llama-cpp), openai, gemini
- **Storage:** SQLite with sqlite-vec
- **Auto-watches** memory files for changes
- **QMD backend** (experimental): BM25 + vectors + reranking

### Pre-Compaction Memory Flush
Automatic silent turn before compaction that reminds model to save important context:
```json
{
  "compaction": {
    "memoryFlush": {
      "enabled": true,
      "softThresholdTokens": 4000,
      "systemPrompt": "Session nearing compaction. Store durable memories now."
    }
  }
}
```

## Model Failover (Built-in)

### Two-Stage Failover
1. **Auth profile rotation** within provider (round-robin, OAuth before API keys)
2. **Model fallback** to next in `agents.defaults.model.fallbacks`

### Cooldowns with Exponential Backoff
- 1 min → 5 min → 25 min → 1 hour (cap)
- Billing failures: 5 hours start, doubles, caps at 24 hours

### Session Stickiness
- Pins auth profile per session for cache warmth
- Only rotates on: session reset, compaction, or profile in cooldown

## Multi-Agent Routing

### Agent Definition
Each agent has isolated:
- **Workspace** (files, AGENTS.md, SOUL.md, USER.md)
- **State directory** (auth profiles, sessions)
- **Skills** (per-agent + shared)

### Binding Rules
```json
{
  "bindings": [
    { "agentId": "work", "match": { "channel": "whatsapp", "accountId": "biz" } },
    { "agentId": "home", "match": { "channel": "telegram" } },
    { "agentId": "opus", "match": { "channel": "whatsapp", "peer": { "kind": "dm", "id": "+15551234567" } } }
  ]
}
```

**Precedence:** peer match > guildId > teamId > accountId > channel > default

### Per-Agent Sandbox & Tools
```json
{
  "agents": {
    "list": [{
      "id": "family",
      "sandbox": { "mode": "all", "scope": "agent" },
      "tools": {
        "allow": ["read", "exec"],
        "deny": ["write", "browser"]
      }
    }]
  }
}
```

## Memory Research Notes (Experimental)

### Proposed Architecture
```
memory/
  bank/                  # Typed memory pages
    world.md            # Objective facts
    experience.md       # What agent did (first-person)
    opinions.md         # Subjective prefs + confidence + evidence
  entities/
    Peter.md
    The-Castle.md
```

### Retain/Recall/Reflect Loop
1. **Retain:** Normalize daily logs into facts with type tags
   - `W` = world fact
   - `B` = biographical/experience
   - `O(c=0.95)` = opinion with confidence
   - `@Entity` mentions

2. **Recall:** Query derived index
   - Lexical (FTS5)
   - Entity-based
   - Temporal
   - Opinion with confidence

3. **Reflect:** Scheduled job to
   - Update entity pages
   - Evolve opinion confidence
   - Propose edits to MEMORY.md

### Opinion Evolution
```json
{
  "statement": "Prefers concise replies",
  "confidence": 0.95,
  "last_updated": "2025-11-27",
  "evidence": ["supporting_fact_ids", "contradicting_fact_ids"]
}
```

## Skills System

### Locations (precedence)
1. `<workspace>/skills` (highest)
2. `~/.openclaw/skills` (managed)
3. Bundled skills (lowest)

### Gating Metadata
```yaml
metadata:
  openclaw:
    requires:
      bins: ["uv"]
      env: ["GEMINI_API_KEY"]
      config: ["browser.enabled"]
    primaryEnv: "GEMINI_API_KEY"
```

### ClawHub
- Registry at clawhub.com
- `clawhub install <skill>`
- `clawhub update --all`

## What We Could Adopt

### Already Have (validated)
- ✅ File-based memory (MEMORY.md + daily logs)
- ✅ Multi-agent routing with bindings
- ✅ Per-agent workspaces

### Could Improve
- ⚡ **Model failover** - Their implementation is more sophisticated
- ⚡ **Memory flush before compaction** - We have pre-compact but could integrate
- ⚡ **Vector memory search** - Built-in with multiple providers
- ⚡ **Opinion confidence evolution** - Novel idea from research

### New Ideas
- 🆕 **QMD backend** - BM25 + vectors + reranking
- 🆕 **Entity pages** in memory/bank/entities/
- 🆕 **Typed facts** with W/B/O prefixes
- 🆕 **Per-agent sandbox & tool restrictions**
