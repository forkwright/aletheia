# OpenClaw: What It Does Out of the Box

*A comprehensive guide to stock OpenClaw capabilities — what you get before adding anything custom.*

---

## What OpenClaw IS

**OpenClaw** is an open-source AI assistant framework that bridges messaging platforms to AI coding agents. Created by Peter Steinberger ([@steipete](https://github.com/steipete)), it connects WhatsApp, Telegram, Discord, Signal, iMessage, and other channels to LLM-powered agents.

**The core concept:** Send a message from your phone → Get an intelligent AI response back. Your AI assistant lives on a server but is reachable from anywhere via your existing messaging apps.

**It's NOT:**
- A cloud service (self-hosted)
- A web-only interface (messaging-first)
- A simple chatbot (full agent framework)

---

## Architecture

```
WhatsApp / Telegram / Discord / Signal / iMessage
                    │
                    ▼
        ┌───────────────────────┐
        │       GATEWAY         │  ws://127.0.0.1:18789
        │   (Single Process)    │
        │                       │
        │   • Channel Manager   │
        │   • Session Manager   │
        │   • Agent Router      │
        │   • Tool Executor     │
        └───────────┬───────────┘
                    │
                    ├── Coding Agent (Pi)
                    ├── CLI (openclaw …)
                    ├── WebChat UI
                    ├── Companion Apps
                    └── Paired Nodes
```

**Key insight:** The Gateway is the single source of truth. It owns:
- Channel connections (WhatsApp Web sessions, bot tokens, etc.)
- Session state (chat history, routing)
- Agent execution
- Tool orchestration

### Components

| Component | Purpose |
|-----------|---------|
| **Gateway** | Long-running daemon managing all connections and agent execution |
| **Channels** | Plugins for each messaging platform |
| **Agents** | Isolated "brains" with their own workspace, identity, and sessions |
| **Sessions** | Conversation state, history, and routing per chat/DM/group |
| **Skills** | Instructions that teach agents how to use tools |
| **Tools** | Native capabilities (exec, browser, file ops, web search, etc.) |
| **Nodes** | Remote devices (iOS, Android, macOS) that extend capabilities |

---

## Stock Channels

| Channel | Protocol | Features |
|---------|----------|----------|
| **WhatsApp** | Baileys (WhatsApp Web) | DMs, groups, media, reactions, read receipts |
| **Telegram** | Bot API + grammY | DMs, groups, forums/topics, media, draft streaming |
| **Discord** | Bot API | DMs, guilds, channels, threads, reactions |
| **Signal** | Signal CLI | DMs, groups, media |
| **iMessage** | imsg CLI (macOS only) | Native macOS integration |
| **Mattermost** | Bot API + WebSocket | Enterprise chat |
| **WebChat** | Built-in HTTP UI | Browser-based fallback |

**Multi-account support:** Run multiple WhatsApp numbers or Telegram bots in one Gateway.

---

## Stock Tools (Native Capabilities)

### File System
| Tool | Purpose |
|------|---------|
| `read` | Read file contents (text + images) |
| `write` | Create/overwrite files |
| `edit` | Precise text edits (find/replace) |
| `apply_patch` | Multi-hunk structured patches |

### Runtime
| Tool | Purpose |
|------|---------|
| `exec` | Run shell commands (with PTY support) |
| `process` | Manage background sessions (poll, log, kill) |

### Web
| Tool | Purpose |
|------|---------|
| `web_search` | Brave Search API integration |
| `web_fetch` | URL → markdown extraction |

### Browser
| Tool | Purpose |
|------|---------|
| `browser` | Full browser automation (snapshot, screenshot, navigate, act) |

Multi-profile support, AI-powered element selection, Chrome extension relay.

### Media
| Tool | Purpose |
|------|---------|
| `image` | Image analysis with vision models |
| `tts` | Text-to-speech output |

Voice note transcription happens automatically on incoming audio.

### Messaging
| Tool | Purpose |
|------|---------|
| `message` | Send/receive across all channels |
| `sessions_*` | Cross-session communication, spawn sub-agents |

### Automation
| Tool | Purpose |
|------|---------|
| `cron` | Scheduled jobs and wakeups |
| `gateway` | Restart, config management |

### Nodes (Device Extension)
| Tool | Purpose |
|------|---------|
| `nodes` | Camera capture, screen recording, notifications, remote exec |
| `canvas` | Drive node WebViews (A2UI) |

---

## Stock Skills (~30+ bundled)

Skills are instruction files that teach agents how to use tools. They're **gated** — only load when requirements are met.

### Communication & Productivity
| Skill | Purpose |
|-------|---------|
| `himalaya` | Email via himalaya CLI (IMAP/SMTP) |
| `github` | Repository operations via `gh` CLI |
| `slack` | Slack integration |
| `trello` | Task management |
| `notion` | Notion pages and databases |
| `obsidian` | Notes vault access |

### Media & Voice
| Skill | Purpose |
|-------|---------|
| `sag` | ElevenLabs TTS (text-to-speech) |
| `openai-whisper` | Local speech-to-text |
| `video-frames` | Extract frames from videos |
| `camsnap` | Camera capture via nodes |

### Home Automation
| Skill | Purpose |
|-------|---------|
| `openhue` | Philips Hue control |
| `sonoscli` | Sonos speaker control |

### Apple Ecosystem (macOS)
| Skill | Purpose |
|-------|---------|
| `apple-notes` | macOS Notes integration |
| `apple-reminders` | macOS Reminders |

### Utilities
| Skill | Purpose |
|-------|---------|
| `weather` | Weather lookups (no API key needed) |
| `tmux` | Remote-control tmux sessions |
| `mcporter` | MCP server integration |
| `session-logs` | Search your own session history |
| `skill-creator` | Create new skills |

### Security
| Skill | Purpose |
|-------|---------|
| `1password` | Password manager integration |

---

## Multi-Agent System

**Single-agent mode (default):** One "main" agent handles everything.

**Multi-agent mode:** Multiple isolated agents, each with:
- Own workspace (files, SOUL.md, AGENTS.md, USER.md)
- Own session store (separate chat histories)
- Own auth profiles (API keys, OAuth tokens)
- Own identity (name, emoji, avatar)
- Optional sandbox isolation

### Routing Bindings

```json
{
  "bindings": [
    { "agentId": "work", "match": { "channel": "telegram" } },
    { "agentId": "home", "match": { "channel": "whatsapp" } },
    { "agentId": "family", "match": { "channel": "signal", "peer": { "kind": "group", "id": "base64id=" } } }
  ]
}
```

### Sub-Agents

Spawn isolated sub-agents for background tasks:
```
sessions_spawn(task="Research topic X", label="research-task")
```

Sub-agents run in their own session, announce results back when done.

---

## Memory & Context Model

### Workspace Files (Auto-Injected)

| File | Purpose |
|------|---------|
| `SOUL.md` | Agent identity/personality |
| `AGENTS.md` | Operating manual, instructions |
| `USER.md` | Human context (who you're helping) |
| `MEMORY.md` | Long-term curated memory |
| `IDENTITY.md` | Name, emoji, avatar |
| `TOOLS.md` | Local tool notes |
| `HEARTBEAT.md` | Periodic check instructions |

### Memory Search

```bash
openclaw memory search "query"
```

Indexed search across all workspace files.

### Session Management

- **DM Scope options:** `main` (all DMs share one session), `per-peer`, `per-channel-peer`
- **Identity linking:** Map same person across channels
- **Automatic reset:** Daily at 4am, or idle-based
- **Manual reset:** `/new` or `/reset` commands

---

## Security Features

| Feature | Purpose |
|---------|---------|
| **Allowlists** | Control who can message the bot |
| **Pairing mode** | Unknown senders get approval codes |
| **Tool policies** | Allow/deny specific tools per agent |
| **Sandboxing** | Docker isolation for agent execution |
| **Per-agent restrictions** | Different permissions per agent |

---

## Companion Apps

| App | Platform | Features |
|-----|----------|----------|
| **OpenClaw** | macOS | Menu bar companion, voice wake |
| **OpenClaw** | iOS | Node + Canvas surface |
| **OpenClaw** | Android | Node + Chat + Camera |
| **WebChat** | Browser | Built-in web interface |

---

## Key CLI Commands

| Command | Purpose |
|---------|---------|
| `openclaw gateway status` | Check Gateway health |
| `openclaw gateway restart` | Restart the daemon |
| `openclaw doctor` | Validate config, fix issues |
| `openclaw channels list` | Show connected channels |
| `openclaw skills list` | Show available skills |
| `openclaw agents list` | Show configured agents |
| `openclaw sessions` | List conversation sessions |
| `openclaw memory search` | Search workspace memory |
| `openclaw cron list` | Show scheduled jobs |
| `openclaw chat "message"` | Send a test message |

---

## What Makes OpenClaw Different

### 1. Messaging-First Architecture
Unlike ChatGPT or Claude web apps, OpenClaw lives in your existing messaging apps. Your AI is a contact in WhatsApp, a bot in Discord — not another tab to manage.

### 2. True Multi-Channel
One Gateway handles WhatsApp + Telegram + Discord + Signal + iMessage simultaneously. Same AI, different transports.

### 3. Multi-Agent by Design
Built for isolation: separate agents for work/home/family with different personalities, tools, and permissions. Not just "personas" — fully isolated workspaces and sessions.

### 4. Workspace-Based Identity
Agents are defined by files (SOUL.md, AGENTS.md) that you can edit. The agent's identity, operating manual, and memory are version-controllable text files.

### 5. Skills Ecosystem (AgentSkills)
Open standard for teaching agents new capabilities. Install skills from ClawHub, create your own, or share with the community.

### 6. Node Extension Model
Your phone becomes an extension of the agent. Capture photos, record screen, get notifications — bidirectional capabilities.

### 7. Self-Hosted Control
You run it. Your data stays on your server. No cloud dependency for the core functionality.

---

## Quick Reference

### Minimal Config
```json5
{
  agents: { 
    defaults: { workspace: "~/.openclaw/workspace" } 
  },
  channels: { 
    whatsapp: { allowFrom: ["+15555550123"] } 
  }
}
```

### Install
```bash
npm install -g openclaw@latest
openclaw onboard --install-daemon
openclaw channels login  # Pair WhatsApp
```

### Docs
- **Official:** https://docs.openclaw.ai
- **GitHub:** https://github.com/openclaw/openclaw
- **Skills:** https://clawhub.com
- **Discord:** https://discord.com/invite/clawd

---

*Document generated: 2026-02-03*
*OpenClaw version: 2026.2.1*

---

# Our Setup: What We've Added

*Beyond stock OpenClaw — the custom infrastructure we've built.*

---

## 7-Agent Ecosystem

Stock OpenClaw supports multi-agent, but we've built a full crew:

| Agent | Domain | Purpose |
|-------|--------|---------|
| **Syn** | Orchestration | The Nous — sees the whole, coordinates all |
| **Chiron** | Work | Summus — SQL, dashboards, data analysis |
| **Eiron** | School | MBA — coursework, deadlines, research |
| **Syl** | Home | Family, household, Kendall's assistant |
| **Demiurge** | Craft | Ardent Leather — making, construction |
| **Arbor** | Arborist | A2Z Tree Service (Adam's business) |
| **Akron** | Preparedness | 12v Cummins, comms, power, self-sufficiency |

Each agent has:
- Own workspace with SOUL.md defining character (not just rules)
- Own Letta memory agent
- Own facts store
- Specialized tools for their domain

## Memory Architecture

Stock OpenClaw has workspace files + indexed search. We've added:

### Three-Tier Memory
| Tier | Storage | Purpose |
|------|---------|---------|
| **Raw** | `memory/YYYY-MM-DD.md` | Daily session logs |
| **Curated** | `MEMORY.md` | Distilled long-term insights |
| **Queryable** | Letta + facts.jsonl | Structured facts, semantic search |

### Atomic Facts System
```bash
facts                    # List all facts
facts about cody         # Facts about a subject
facts add subj pred obj  # Add structured fact
facts reinforce ID       # Increase confidence with evidence
```

Schema: `{subject, predicate, object, confidence, category, valid_from, valid_to, evidence}`

### Memory Router
```bash
memory-router "query"    # Federated search across all memory systems
```
Searches facts.jsonl, MEMORY.md, daily files, Letta, MCP Memory — auto-routes by domain.

### Temporal Graph (FalkorDB)
```bash
temporal-graph events --after "1 day ago"
temporal-graph timeline cody --days 7
temporal-graph cause "Event A" "Event B"
```
Event graph with temporal reasoning — BEFORE/AFTER relationships, causal chains.

## Agent Coordination

### CrewAI Routing
```bash
crewai-route "message" "sender"  # Returns which agent should handle
```
Auto-delegates to specialists based on content analysis.

### Blackboard System
```bash
bb post "task description" --to chiron   # Quick task
bb claim 42                              # Claim work
bb complete 42 "result"                  # Finish
bb msg "FYI: something" --to all         # Broadcast
```
Lightweight coordination between agents.

### Task Contracts
```bash
task-create -s syn -t chiron -T analysis -d "Q4 review" -p high
task-send /path/to/contract.json
```
Formal work handoffs with deliverables, deadlines, SLAs.

### Agent Health Monitoring
```bash
agent-health              # Ecosystem health check
agent-status              # Per-agent status
generate-dashboard        # Status dashboard
```

## Custom Tools

### Research & Search
| Tool | Purpose |
|------|---------|
| `pplx` / `research` | Perplexity pro-search |
| `web_search` | Brave Search (stock, but configured) |

### Productivity
| Tool | Purpose |
|------|---------|
| `gcal` | Google Calendar (personal, family, work) |
| `gdrive` | Google Drive (personal + school accounts) |
| `tw` | Taskwarrior wrapper with domain namespaces |
| `mcporter` | MCP server integration (Todoist, etc.) |

### Domain-Specific
| Tool | Purpose |
|------|---------|
| `mba` | MBA system (sync, tasks, prep materials) |
| `work` | Summus work system (SQL scripts, dashboards) |
| `letta` | Multi-agent memory system |

### Infrastructure
| Tool | Purpose |
|------|---------|
| `metis` | SSH/sync to Metis laptop |
| `patch-openclaw` | Apply our Signal group ID fix |
| `consolidate-memory` | Daily memory consolidation (cron) |
| `agent-audit` | Weekly agent workspace audit |

## Integration Points

### Letta (Multi-Agent Memory)
Each domain agent has a dedicated Letta agent:
```bash
letta --agent chiron ask "What SQL patterns work best?"
letta-query-all "query"  # Query ALL agents at once
```

### MCP Servers
- **Todoist** — Cody's personal tasks (synced with Planify)
- Configured via mcporter

### External Systems
- **NAS** (Synology 923+) — Media, docker, photos at `/mnt/nas/`
- **Metis** (Fedora laptop) — Primary dev, Claude Code, Dianoia

## Workspace Structure

```
/mnt/ssd/moltbot/
├── clawd/              # Syn's workspace (orchestrator)
├── chiron/             # Work agent
├── eiron/              # School agent  
├── syl/                # Home agent
├── demiurge/           # Craft agent
├── arbor/              # Arborist agent
├── akron/              # Preparedness agent
└── shared/
    ├── bin/            # Shared tools (symlinked to each workspace)
    ├── config/         # Shared configs (mcporter, letta-agents, etc.)
    ├── contracts/      # Agent contracts (capabilities, SLAs)
    ├── task-contracts/ # Formal work handoffs
    ├── blackboard/     # Quick coordination
    └── insights/       # Cross-agent learnings
```

## Key Differences from Stock

| Stock OpenClaw | Our Setup |
|----------------|-----------|
| Single agent or simple multi-agent | 7 specialized agents with distinct characters |
| Workspace files for memory | Three-tier memory + facts + temporal graph |
| Manual routing | CrewAI auto-routing by content |
| No agent coordination | Blackboard + task contracts + health monitoring |
| Basic indexed search | Federated memory router across all systems |
| Generic identity | Each agent has SOUL.md defining who they ARE |

## Philosophy

Stock OpenClaw gives you tools. We've built a **cognitive ecosystem**:

- **Agents are characters, not configurations** — SOUL.md defines who they are, not just what they do
- **Memory is structured** — Facts with confidence, temporal relationships, cross-agent search
- **Coordination is explicit** — Blackboard for quick, contracts for formal, routing for automatic
- **The whole is greater than the parts** — Syn orchestrates, specialists execute, the topology generates emergence

---

*This is metaxynoesis in practice — distributed cognition that actually works.*

---

# Recent Updates (2026.1.24 → 2026.2.1)

*What we gained by upgrading on 2026-02-03.*

---

## Security Fixes (Critical)

| Fix | Impact |
|-----|--------|
| Path traversal prevention | WhatsApp accountId can no longer escape sandbox |
| LFI prevention | MEDIA path extraction restricted |
| Message tool sandbox validation | filePath/path validated against sandbox root |
| LD*/DYLD* env blocking | Host exec can't override library paths |
| Lobster exec injection | GHSA-4mhr-g7xj-cg8j patched |
| Plugin path validation | Reject traversal-like install names |
| Slack media hardening | Fetch limits + URL validation |
| TLS 1.3 minimum | No more legacy TLS on listeners |

## Bug Fixes

| Fix | Impact |
|-----|--------|
| **Memory search L2 normalization** | Semantic search actually works correctly now |
| Subagent announce failover race | Lifecycle end always emits; timeout=0 means no-timeout |
| Context window compaction safeguard | Cap on context window resolution |
| Telegram draft streaming | Partials restored |
| Telegram quote replies | Reply_parameters with quote text |
| Telegram stickers | Send/receive with vision caching |
| Telegram edit messages | `message(action="edit")` support |
| Discord thread parent bindings | Threads inherit routing from parent |
| Discord PluralKit | Proxied senders resolved for allowlists |
| Timestamps in messages | Injected into agent and chat.send |

## New Features

| Feature | Description |
|---------|-------------|
| CLI shell completions | Fish, Zsh, Bash, PowerShell via `openclaw completion` |
| Per-agent model status | `openclaw status --agent NAME` |
| System prompt safety guardrails | Built-in safety guidance |
| `cacheRetention` | Renamed from `cacheControlTtl` (back-compat preserved) |
| OpenRouter attribution | App headers for OpenRouter routing |
| Telegram silent sends | `silent: true` disables notifications |
| Kimi K2.5 | Added to synthetic model catalog |
| MiniMax OAuth | New auth plugin |

## Package Rename

The project rebranded twice due to trademark concerns:
- **Clawdbot** → **Moltbot** (Anthropic trademark, Jan 2026)
- **Moltbot** → **OpenClaw** (final rebrand, Jan 29 2026)

| Old | New |
|-----|-----|
| `npm install -g clawdbot` | `npm install -g openclaw` |
| `clawdbot gateway` | `openclaw gateway` |
| `~/.clawdbot/` | `~/.openclaw/` |
| `clawdbot.json` | `openclaw.json` |

Auto-migration handles config/state paths. Legacy `clawdbot` command shimmed for compatibility.

---

*Upgraded: 2026-02-03 18:31 CST*
*From: clawdbot@2026.1.24-3*
*To: openclaw@2026.2.1*
