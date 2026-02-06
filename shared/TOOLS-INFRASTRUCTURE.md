# Shared Infrastructure Documentation

> This file documents infrastructure available to ALL agents in the Aletheia ecosystem.
> Location: `/mnt/ssd/aletheia/shared/TOOLS-INFRASTRUCTURE.md`

## Network Topology

```
┌─────────────────────────────────────────────────────────────────┐
│                        Home Network                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐      │
│  │ server-host  │    │    Laptop     │    │   nas-host   │      │
│  │ SERVER_IP │◄──►│ LAPTOP_IP │    │NAS_IP │      │
│  │   (Ubuntu)   │    │   (Fedora)   │    │  (Synology)  │      │
│  │              │    │              │    │              │      │
│  │ • Clawdbot   │    │ • Claude Code│    │ • 32TB NAS   │      │
│  │ • 5 Agents   │    │ • Dianoia    │    │ • Media      │      │
│  │ • Docker     │    │ • Dev env    │    │ • Docker     │      │
│  └──────┬───────┘    └──────────────┘    └──────┬───────┘      │
│         │                                        │               │
│         └────────────── NFS Mounts ─────────────┘               │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘

Tailscale IPs (accessible from anywhere):
  • server-host: TAILSCALE_SERVER
  • nas-host:    TAILSCALE_NAS
  • laptop:       TAILSCALE_LAPTOP
```

## Storage Layout

### Worker Node (This Server)

| Path | Type | Purpose |
|------|------|---------|
| `/mnt/ssd/aletheia/` | Local SSD | Agent workspaces, all aletheia data |
| `/mnt/ssd/aletheia/shared/` | Local | Cross-agent shared resources |
| `/mnt/ssd/aletheia/nous/syn/` | Local | Syn's workspace |
| `/mnt/ssd/aletheia/nous/chiron/` | Local | Chiron's workspace (work) |
| `/mnt/ssd/aletheia/nous/eiron/` | Local | Eiron's workspace (school) |
| `/mnt/ssd/aletheia/nous/syl/` | Local | Syl's workspace (home) |
| `/mnt/ssd/aletheia/nous/demiurge/` | Local | Demiurge's workspace (craft) |

### NAS Mounts (NFS)

| Mount Point | NAS Path | Purpose |
|-------------|----------|---------|
| `/mnt/nas/Media` | NAS_DATA/media | Movies, TV, Music |
| `/mnt/nas/photos` | NAS_HOMES/the operator.forkwright/Photos | Photo library |
| `/mnt/nas/home` | NAS_HOMES/the operator.forkwright | the operator's NAS home |
| `/mnt/nas/docker` | NAS_DATA/docker | Docker persistent data |
| `/mnt/nas/vpn_media` | NAS_DATA/docker/vpn_media | VPN-routed downloads |

### Laptop (Fedora Laptop)

| Path | Purpose | Access |
|------|---------|--------|
| `/home/ck/dianoia/` | the operator's meta-system | SSH as syn (group access) |
| `/home/ck/dianoia/state/dianoia.db` | SQLite: tasks, sessions | Read/write via syn |
| `/home/syn/` | syn user home | Full access |

## SSH Access

### Configuration (`~/.ssh/config`)

```
Host laptop
    HostName LAPTOP_IP
    User syn
    BatchMode yes

Host nas nas-host
    HostName NAS_IP
    User syn
    BatchMode yes
```

### Usage

```bash
# Laptop commands
ssh laptop "command"           # Run command
ssh -t laptop                  # Interactive shell
scp laptop:path/file ./        # Copy from Laptop
scp ./file laptop:path/        # Copy to Laptop

# NAS commands (if SSH enabled)
ssh nas "command"
```

### Laptop Helper Tool

```bash
laptop ssh [cmd]       # Run command or interactive shell
laptop claude [args]   # Run Claude Code on Laptop
laptop projects        # Show dianoia projects.json
laptop db [query]      # Query dianoia.db
laptop inbox           # List dianoia inbox
laptop sync <r> [l]    # Copy file from Laptop
laptop push <l> [r]    # Copy file to Laptop
```

## Shared Tools

All tools in `/mnt/ssd/aletheia/shared/bin/` are symlinked to each agent's workspace.

### Core Tools

| Tool | Purpose |
|------|---------|
| `gcal` | Google Calendar CLI |
| `gdrive` | Google Drive CLI |
| `tw` | Taskwarrior wrapper |
| `letta` | Agent memory system |
| `facts` | Atomic facts management |
| `memory-router` | Federated memory search |

### Agent Coordination

| Tool | Purpose |
|------|---------|
| `agent-health` | Monitor ecosystem health |
| `agent-status` | Aggregated status view |
| `bb` (blackboard) | Quick coordination/messaging |
| `task-create` | Create formal task contracts |
| `task-send` | Send tasks to agents |

### Research & External

| Tool | Purpose |
|------|---------|
| `pplx` / `research` | Perplexity search |
| `moltbook-feed` | Fetch Moltbook posts |
| `provider-health` | LLM provider status |
| `skill-audit` | Security audit for skills |

### Utilities

| Tool | Purpose |
|------|---------|
| `laptop` | SSH helper for Laptop |
| `update-llms-txt` | Regenerate agent docs |
| `pre-compact` | Context preservation |
| `extract-insights` | Extract insights from conversations |
| `coding-agent` | Control Claude Code via tmux |

## Configuration Files

| File | Purpose |
|------|---------|
| `/mnt/ssd/aletheia/shared/config/mcporter.json` | MCP server configurations |
| `/mnt/ssd/aletheia/shared/config/letta-agents.json` | Letta agent mappings |
| `/mnt/ssd/aletheia/shared/config/provider-failover.json` | Multi-provider routing |

## Memory Systems

### Per-Agent Memory

Each agent workspace contains:
- `MEMORY.md` — Curated long-term insights
- `memory/*.md` — Daily session logs
- `memory/heartbeat-state.json` — State tracking

### Shared Memory

| System | Location | Purpose |
|--------|----------|---------|
| `facts.jsonl` | shared/memory/ | Structured facts with confidence |
| Letta | Letta server | Agent-specific memory stores |
| MCP Memory | MCP server | Knowledge graph |

### Federated Search

```bash
memory-router "query"                    # Auto-routes by domain
memory-router "query" --domains all      # Search everywhere
memory-router "query" --sources facts    # Specific sources
```

### Entity Pages & Reflection

Entity pages live in `memory/entities/*.md` — auto-generated summaries per entity.

```bash
reflect --list              # List all entities with fact counts
reflect --entity the operator       # Generate page for specific entity
reflect --all               # Generate pages for all entities
reflect --stats             # Show fact statistics
```

Run `reflect --all` periodically to keep entity pages fresh.

### Opinion Confidence Evolution

Facts support confidence evolution with evidence tracking:

```bash
facts reinforce <id> -e "evidence"   # Increase confidence (+10% toward 1.0)
facts contradict <id> -e "evidence"  # Decrease confidence (-15%)
facts conflicts                       # Find potentially conflicting facts
facts review                          # Show facts flagged for review
```

**How confidence evolves:**
- Facts below 30% confidence are flagged for review
- Each fact can have `supporting_evidence` and `contradicting_evidence` arrays

## Model Failover (Built-in)

Clawdbot now has automatic model failback:
- **Primary:** claude-opus-4-5
- **Fallbacks:** claude-sonnet-4 → claude-haiku-3.5

If primary fails (rate limit, outage), automatically tries next in chain with exponential backoff cooldowns.

## Memory Flush Before Compaction (Built-in)

When session approaches context limit, Clawdbot triggers a silent turn prompting the model to save important context before compaction wipes it.

Enabled automatically — no action needed.

## Docker Services

Running on server-host (managed via Portainer):

| Service | Port | Purpose |
|---------|------|---------|
| Clawdbot Gateway | 8443 | Agent API/webchat |
| Letta | 8383 | Memory system |
| Portainer | 9443 | Container management |
| FalkorDB | 6379 | Graph database |
| Ollama | 11434 | Local LLMs |

## External APIs

| Service | Key Location | Purpose |
|---------|--------------|---------|
| Anthropic | ANTHROPIC_API_KEY | Primary LLM |
| OpenRouter | OPENROUTER_API_KEY | Fallback LLM |
| OpenAI | OPENAI_API_KEY | Fallback LLM |
| Perplexity | PERPLEXITY_API_KEY | Research |
| Google AI / Gemini | shared/config/api-keys.env | Long context, cheap queries |
| Moltbook | heartbeat-state.json | Community intel |

---

## ⚠️ RESEARCH-FIRST PROTOCOL (MANDATORY)

**Before self-reasoning on factual questions, USE EXTERNAL TOOLS:**

1. **Perplexity FIRST** for any factual/research question:
   ```bash
   pplx "your question here" --sources
   ```

2. **Memory search** for anything about the operator, the system, or past decisions:
   ```bash
   memory-router "query"
   ```

3. **Web search** for current events, pricing, technical docs:
   ```bash
   # Built into Clawdbot
   web_search "query"
   ```

**Why:** Claude tokens are expensive. Perplexity is cheap ($0.03/query). Don't burn context and money reasoning about things you can look up.

**What to self-reason:** Judgment calls, synthesis, creative work, orchestration, nuanced decisions. That's what Claude is FOR.

---

## Google Gemini API

**Key location:** `/mnt/ssd/aletheia/shared/config/api-keys.env`

### Available Models

| Model | Best For | Pricing |
|-------|----------|---------|
| `gemini-2.5-pro` | Complex reasoning, long docs | $1.25-2.50/$10-15 per 1M tok |
| `gemini-2.5-flash` | Fast, cheap general use | $0.30/$2.50 per 1M tok |
| `gemini-2.0-flash-lite` | Cheapest option | Very low |

### When to Use Gemini

| Task | Route to |
|------|----------|
| Long document analysis (>200K tokens) | Gemini 2.5 Pro (1M context) |
| Simple/routine queries | Gemini Flash |
| Bulk/batch processing | Gemini Flash |
| Nuanced reasoning, judgment | Claude (stay here) |

### Direct API Usage

```bash
# Load API key
source /mnt/ssd/aletheia/shared/config/api-keys.env

# Simple query
curl -s "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key=$GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"contents":[{"parts":[{"text":"Your prompt here"}]}]}' | jq -r '.candidates[0].content.parts[0].text'
```

### Cost Comparison (per 1M tokens)

| Model | Input | Output | Context |
|-------|-------|--------|---------|
| Claude Opus 4.5 | ~$15 | ~$75 | 200K |
| Gemini 2.5 Pro | $1.25 | $10 | 1M |
| Gemini 2.5 Flash | $0.30 | $2.50 | 1M |
| Claude Haiku | $0.25 | $1.25 | 200K |

**Rule of thumb:** If it doesn't need Claude's judgment, don't use Claude's tokens.

---

## Agent Domains

| Agent | Domain | Taskwarrior Project | Letta Agent |
|-------|--------|---------------------|-------------|
| Syn | Orchestration | project:infra | syn |
| Chiron | Work (Summus) | project:work | chiron |
| Eiron | School (MBA) | project:school | eiron |
| Syl | Home/Family | project:home | syl |
| Demiurge | Craft/Making | project:craft | demiurge |

---

*Last updated: 2026-02-03*
*Location: /mnt/ssd/aletheia/shared/TOOLS-INFRASTRUCTURE.md*

## Memory Router (Upgraded 2026-02-03)

Federated memory search with iterative retrieval.

| Command | What it does |
|---------|--------------|
| `memory-router "query"` | Iterative search with synonym expansion |
| `memory-router "query" --domains X` | Search specific domain |
| `memory-router "query" --debug` | Show expansion and iteration details |
| `memory-router "query" --json` | Output as JSON |
| `memory-router "query" --no-iterative` | Single-pass (old behavior) |

**Features:** Synonym expansion, concept clustering, multi-strategy (exact + semantic), result fusion, early stopping.

Old version preserved as `memory-router-v1`.
