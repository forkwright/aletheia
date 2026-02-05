# Agent Infrastructure Improvements

*Analysis of Arbor and Demiurge issues, with long-term solutions.*

---

## Issues Observed

### Arbor (New Agent)

| Issue | Symptom | Root Cause |
|-------|---------|------------|
| Tone not sticking | Kept using caps/emojis after corrections | SOUL.md not re-read after session clear; tone rules too buried |
| Memory search failed | "No API key for openai/google" | Ollama not configured in agents.defaults |
| Session appeared frozen | No response in Signal group | Unknown - had to clear session manually |
| Missing core files | IDENTITY.md, TOOLS.md, MEMORY.md empty/template | New agent bootstrap incomplete |
| Research rules missing | Could make things up without checking | Not explicit in SOUL.md |
| Judgment protocol missing | No guardrails for Adam requests | Not defined until added manually |
| Photo access blocked | Permission denied on Metis | syn user can't read ck's home dir |

### Demiurge (Established Agent)

| Issue | Symptom | Root Cause |
|-------|---------|------------|
| Cross-domain work | Got pulled into Cloudflare debugging for A2Z | No clear domain boundary enforcement |
| Memory search empty initially | Index not populated | Memory files existed but not indexed |

---

## Root Causes

1. **Session Clear = Memory Loss**
   - When session is cleared, agent loses all in-context guidance
   - SOUL.md may not be re-read immediately
   - Tone/style rules need to be earlier and more prominent

2. **New Agent Bootstrap Is Manual**
   - Core files left as templates
   - No checklist or automation
   - Each agent setup requires manual intervention

3. **Memory Search Not Pre-Configured**
   - Default tried OpenAI/Google, failed without keys
   - Ollama was available but not configured
   - Should be in agents.defaults

4. **Cross-Workspace Access Is Clunky**
   - SSH + sudo required for Metis files
   - No shared mount or sync automation
   - Each request is manual

5. **Domain Boundaries Not Enforced**
   - Agents respond to any request that reaches them
   - No explicit "this is not my domain, routing to X"
   - Demi helped with Cloudflare because asked, not because it was their domain

---

## Long-Term Solutions

### 1. New Agent Bootstrap Script

Create `bin/new-agent` that:
- Creates workspace directory structure
- Populates IDENTITY.md with prompts (not just template)
- Creates MEMORY.md with initial structure
- Populates TOOLS.md with shared tool references
- Copies AGENTS.md from a proper template
- Creates SOUL.md with placeholder sections but required structure
- Adds tone/style rules by default
- Adds research/verification rules by default
- Configures Clawdbot binding

```bash
new-agent arbor --workspace /mnt/ssd/moltbot/arbor --model sonnet
```

### 2. Tone/Style in System Prompt

Add to `agents.defaults` in openclaw.json:
```json
{
  "agents": {
    "defaults": {
      "systemPromptAppend": "Tone: No ALL CAPS. Minimal emojis. Professional, not casual. Let content carry weight."
    }
  }
}
```

This ensures tone survives session clears.

### 3. Session Start File Loading

Ensure agents ALWAYS read on first turn:
- SOUL.md (identity)
- AGENTS.md (operations)
- memory/YYYY-MM-DD.md (recent context)

Add to AGENTS.md template:
```markdown
## Every Session Start
Before responding to any message:
1. Read SOUL.md
2. Read memory/YYYY-MM-DD.md (today and yesterday)
3. Check if this request is in your domain
```

### 4. Memory Search Defaults

Already fixed — add to documentation:
```json
{
  "agents": {
    "defaults": {
      "memorySearch": {
        "enabled": true,
        "provider": "openai",
        "model": "nomic-embed-text",
        "remote": {
          "baseUrl": "http://localhost:11434/v1/",
          "apiKey": "ollama"
        },
        "fallback": "none"
      }
    }
  }
}
```

### 5. Domain Routing in AGENTS.md

Add explicit domain boundaries:
```markdown
## My Domain
- [list what this agent handles]

## Not My Domain (Route to Others)
- Work/SQL → Chiron
- School/MBA → Eiron
- Craft/Leather → Demiurge
- Home/Family → Syl
- Infrastructure/Unclear → Syn

If a request is not in my domain, acknowledge and route:
"This looks like [domain] work — routing to [agent]."
```

### 6. Session Health Monitoring

Create `bin/agent-sessions` that shows:
- Last activity time per agent
- Token count / context usage
- Any error states
- "Possibly stuck" warnings (>10min since last response)

Add to HEARTBEAT.md:
```markdown
### Agent Health Check
Every 4th heartbeat, run:
```bash
agent-sessions --check-health
```
If any agent shows issues, investigate or notify.
```

### 7. Metis File Access

Options:
a) **Shared NFS/SMB mount** — Mount Metis home to worker-node
b) **Sync script** — Periodic rsync of common directories
c) **On-demand SSH** — Current approach, but with sudo pre-configured

Recommended: Create `/mnt/metis/` mount point with read access to common dirs.

### 8. Post-Session-Clear Recovery

When a session is cleared, the next message should trigger:
1. Read SOUL.md
2. Read AGENTS.md
3. Read recent memory files
4. Acknowledge fresh start

Add to AGENTS.md:
```markdown
## Fresh Session Detection
If you have no prior context in this conversation:
1. Read SOUL.md immediately
2. Read AGENTS.md
3. Read memory/YYYY-MM-DD.md
4. Say: "Fresh session. I've loaded my context."
```

---

## Implementation Priority

| Priority | Item | Effort | Impact |
|----------|------|--------|--------|
| 1 | System prompt tone append | 10 min | High - survives session clears |
| 2 | AGENTS.md template with domain routing | 30 min | High - prevents cross-domain drift |
| 3 | New agent bootstrap script | 2 hr | High - prevents incomplete setup |
| 4 | Session health monitoring | 1 hr | Medium - early stuck detection |
| 5 | Metis mount | 30 min | Medium - file access convenience |
| 6 | Fresh session recovery protocol | 15 min | Medium - context restoration |

---

## Immediate Actions

1. **Add systemPromptAppend for tone** to openclaw.json
2. **Update AGENTS.md template** with domain routing section
3. **Document the bootstrap checklist** even before scripting

---

*Created: 2026-01-31*
*For discussion with Cody*
