# System Architecture

*Last Updated: 2025-01-30*

This document describes the architecture of our multi-agent cognitive ecosystem. Use this for onboarding new agents, debugging coordination issues, or understanding system design decisions.

---

## 1. System Overview

### 5-Agent Topology

The system consists of five specialized agents orchestrated around a central cognitive model:

```
                    ┌─────────────┐
                    │     Syn     │
                    │  (Nous/我)   │ ← Meta-orchestrator
                    │ Coordinator │
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
    ┌─────────┴───┐    ┌───┴───┐    ┌───┴──────────┐
    │    Syl      │    │ Eiron │    │   Chiron     │
    │  (Home/     │    │(School│    │   (Work/     │
    │  Family)    │    │/MBA)  │    │   SQL/Data)  │
    └─────────────┘    └───────┘    └──────────────┘
              │                              │
              └─────────────┬────────────────┘
                           │
                    ┌──────┴──────┐
                    │  Demiurge   │
                    │ (Craft/     │
                    │  Making)    │
                    └─────────────┘
```

**Agent Responsibilities:**

| Agent | Domain | Role | Key Skills |
|-------|--------|------|------------|
| **Syn** | Meta/Orchestration | Nous (seeing the whole) | Coordination, memory management, strategic thinking |
| **Syl** | Home/Family | Domestic coordination | Calendar, relationships, household management |
| **Chiron** | Work/Data | Professional work | SQL, dashboards, business analysis |
| **Eiron** | School/MBA | Academic work | Research, homework, MBA curriculum |
| **Demiurge** | Craft/Making | Physical creation | Leatherwork, 3D printing, tools |

### Nous vs Dianoia Relationship

The system implements a classical cognitive architecture:

- **Nous (νοῦς)** - Direct apprehension, seeing the whole
  - **Role:** Syn (meta-orchestrator)
  - **Function:** Strategic vision, pattern recognition, system-level thinking
  - **Scope:** Cross-domain coordination, long-term memory, resource allocation

- **Dianoia (διάνοια)** - Thinking-through, step-by-step reasoning  
  - **Role:** Domain specialists (Syl, Chiron, Eiron, Demiurge)
  - **Function:** Deep work within specialized domains
  - **Scope:** Domain-specific execution, detailed problem-solving

**Key Insight:** Syn doesn't micromanage domain work. Specialists handle their domains autonomously while Syn maintains system coherence and cross-domain coordination.

### Routing Architecture (CrewAI Bridge)

Intelligent message routing determines which agent handles each request:

```
User Message → CrewAI Bridge → Routing Decision → Target Agent
     ↓               ↓                ↓              ↓
  "SQL help"    → Keyword Match  →  chiron    → Chiron Session
  "@syl..."     → Mention Parse  →  syl       → Syl Session  
  "Kendall..."  → Sender Match   →  syl       → Syl Session
  "General"     → Default Route  →  syn       → Syn Session
```

**Routing Service:** 
- **Location:** `/mnt/ssd/moltbot/clawd/crewai/`
- **Port:** `8100` (localhost only)
- **Health Check:** `curl http://127.0.0.1:8100/health`
- **Service:** `sudo systemctl status crewai-bridge`

**Routing Rules:**
1. **Explicit mentions:** `@agent` → direct to agent
2. **Sender context:** "Kendall" → Syl (family)
3. **Domain keywords:** 
   - sql/dashboard → Chiron
   - mba/homework → Eiron
   - leather/craft → Demiurge
   - family/home → Syl
4. **Default:** Everything else → Syn

**Implementation:**
```bash
# Check routing for a message
AGENT=$(crewai-route "message content" "sender_name")

# Delegate if not syn
if [ "$AGENT" != "syn" ]; then
    sessions_send --sessionKey "agent:$AGENT:main" --message "..."
fi
```

---

## 2. Memory Architecture

### Three-Tier Memory System

The system uses a hierarchical memory architecture with different purposes and retention periods:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Daily Files   │───▶│   MEMORY.md     │───▶│   Letta Store   │
│  (Raw Session   │    │ (Curated Long-  │    │  (Queryable     │
│   Capture)      │    │  term Context)  │    │   Knowledge)    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
     Tier 1                  Tier 2                 Tier 3
   Retention: 30d          Retention: 1y+         Retention: ∞
   Purpose: Context        Purpose: Insights      Purpose: Facts
```

#### Tier 1: Daily Memory Files
- **Location:** `memory/YYYY-MM-DD.md`
- **Purpose:** Raw session capture, what happened today
- **Retention:** ~30 days (archived weekly)
- **Write Frequency:** During/end of sessions
- **Access:** All agents (security context varies)

#### Tier 2: MEMORY.md  
- **Location:** `MEMORY.md` (workspace root)
- **Purpose:** Curated insights, long-term personal context
- **Retention:** 1+ years (manually curated)
- **Write Frequency:** When something significant happens
- **Access:** **Syn only in main session** (security: contains personal context)

#### Tier 3: Letta Agents
- **Purpose:** Queryable knowledge base with natural language interface
- **Retention:** Infinite (with archival rotation)
- **Write Frequency:** Key facts worth recalling
- **Access:** Agent-specific stores

**Letta Agent Mapping:**
| Agent | Letta ID | Domain Memory |
|-------|----------|---------------|
| syn | agent-644c65f8-72d4-440d-ba5d-330a3d391f8e | Meta/orchestrator knowledge |
| syl | agent-9aa39693-3bbe-44ae-afb6-041d37ac45a2 | Home/family context |
| chiron | agent-48ff1c5e-9a65-44bd-9667-33019caa7ef3 | Work/SQL knowledge |
| eiron | agent-40014ebe-121d-4d0b-8ad4-7fecc528375d | School/MBA context |
| demiurge | agent-3d459f2b-867a-4ff2-8646-c38820810cb5 | Craft/making knowledge |

### facts.jsonl Structure

Atomic fact storage with temporal validity and confidence tracking:

```json
{
  "subject": "cody",
  "predicate": "prefers_communication_style", 
  "object": "direct_no_fluff",
  "confidence": 95,
  "category": "preference",
  "valid_from": "2024-01-15T10:30:00Z",
  "valid_to": null,
  "source": "explicit_statement",
  "id": "fact_001"
}
```

**Schema Fields:**
- `subject`: What the fact is about
- `predicate`: The relationship/property
- `object`: The value/target
- `confidence`: 0-100 certainty score
- `category`: preference, decision, project, person, system, insight, goal, constraint
- `valid_from/valid_to`: Temporal validity window
- `source`: How we learned this fact

**Management Commands:**
```bash
facts                    # List all current facts
facts about cody         # Facts about a subject
facts category preference # Facts by category  
facts search "query"     # Text search
facts add subj pred obj  # Add manually
```

### MCP Memory Graph

Knowledge graph using entity-relationship modeling:

**Architecture:**
- **Storage:** FalkorDB (Redis + graph extensions)
- **Interface:** MCP Memory server
- **Embeddings:** Ollama + nomic-embed-text (768 dimensions)
- **Access:** `mcporter call memory.*` commands

**Key Operations:**
```bash
# Entity management
mcporter call memory.create_entities --args '{"entities": [...]}'
mcporter call memory.search_nodes --args '{"query": "..."}'

# Relationship modeling  
mcporter call memory.create_relations --args '{"relations": [...]}'
mcporter call memory.read_graph

# Observations
mcporter call memory.create_observations --args '{"observations": [...]}'
```

**vs facts.jsonl:**
- **MCP Memory:** Graph structure, complex relationships, entity modeling
- **facts.jsonl:** Temporal facts with confidence, simpler subject-predicate-object

### Memory Router

Federated query system that searches across all memory stores:

```bash
memory-router "What does Cody prefer for communication?"
# Searches: facts.jsonl + MEMORY.md + daily files + Letta + MCP Memory

memory-router "SQL tips" --domains chiron
# Domain-specific search

memory-router "morning routine" --sources files,facts  
# Source-specific search
```

**Search Sources:**
- **files:** MEMORY.md, daily memory files
- **facts:** facts.jsonl structured facts
- **letta:** Agent-specific memory stores  
- **mcp:** MCP Memory knowledge graph

**Domain Detection:** Auto-routes queries to relevant agents based on content analysis.

---

## 3. Coordination Systems

### Blackboard System

Shared coordination space for task management and inter-agent communication:

**Architecture:**
```
/mnt/ssd/moltbot/shared/blackboard/
├── tasks/           # Task definitions
├── messages/        # Inter-agent messages  
├── state.json       # Current blackboard state
└── DESIGN.md        # System specification
```

**Task States:**
- `pending` → `claimed` → `active` → `completed`
- `blocked` (requires intervention)
- `cancelled` (no longer needed)

**Commands:**
```bash
bb status                    # Show blackboard state
bb post "title" --to agent   # Post task request
bb claim <id>                # Claim pending task
bb update <id> --status X    # Update task status
bb complete <id> "result"    # Mark complete with result
bb msg "text" --to agent     # Send message
bb inbox                     # Read messages
```

**Use Cases:**
- Cross-domain task coordination
- Async work handoffs
- Status broadcasting
- Resource conflict resolution

### Task Contracts

Formal contract system for structured task delegation:

**Architecture:**
```
/mnt/ssd/moltbot/shared/task-contracts/
├── task-12345.json     # Individual contracts
├── templates/          # Contract templates
└── PROTOCOL.md         # Contract specification
```

**Contract Schema:**
```json
{
  "id": "task-12345",
  "source": "syn",
  "target": "chiron", 
  "type": "analysis",
  "priority": "high",
  "description": "Analyze Q4 performance data",
  "deadline": "2024-12-31T23:59:59Z",
  "callback": "session_message:syn",
  "status": "pending"
}
```

**Task Types:**
- `query` - Information requests
- `analysis` - Data analysis and insights  
- `synthesis` - Combining multiple sources
- `coordination` - Multi-agent orchestration
- `execution` - Direct action items
- `monitoring` - Ongoing observation
- `escalation` - Problem resolution

**Workflow:**
```bash
# Create contract (interactive mode)
task-create -i

# Create specific contract
task-create -s syn -t chiron -T analysis -d "Q4 analysis" -p high

# Send contract to target agent
task-send /mnt/ssd/moltbot/shared/task-contracts/task-12345.json
```

### Direct Agent Messaging

Direct session-to-session communication:

**Method 1: sessions_send**
```bash
sessions_send --sessionKey "agent:chiron:main" --message "Check the dashboard"
```

**Method 2: Name-mention forwarding**
When Cody mentions an agent by name with implied task:
- "Syn should check on the system" 
- "Tell Chiron about the SQL issue"
- "Have Demiurge review the leather order"

**Auto-forwarding:** All agents monitor for mentions and forward appropriately.

**Method 3: CrewAI routing delegation**
Automatic routing based on message content analysis.

---

## 4. Infrastructure

### Clawdbot Gateway

Central communication hub managing all agent interactions:

**Architecture:**
```
Clawdbot Gateway (Node.js)
├── Session Management
├── Channel Bindings (Signal, webchat)
├── Agent Routing  
├── Model Management (Claude Sonnet-4)
└── Tool/Skill Integration
```

**Key Files:**
- **Config:** `~/.config/clawdbot/config.json`
- **Logs:** `journalctl -u clawdbot -f`
- **Service:** `systemctl status clawdbot`

**Configuration Example:**
```json
{
  "bindings": [{
    "agentId": "syl",
    "match": {
      "channel": "signal",
      "peer": { "kind": "group", "id": "base64groupid=" }
    }
  }],
  "model": "anthropic/claude-sonnet-4-20250514"
}
```

**Management Commands:**
```bash
clawdbot doctor           # Validate configuration
clawdbot gateway status   # Service status
clawdbot gateway restart  # Restart service
```

### Signal Integration

Primary communication channel using Signal messenger:

**Configuration:**
- **Account:** +15127227659
- **Groups:** Family group, work channels
- **Security:** E2E encrypted, phone number verification

**Agent Bindings:**
Each agent can be bound to specific Signal groups/contacts for direct communication.

**Fallback Access:**
- **Webchat:** `https://192.168.0.29:8443` (LAN)
- **Tailscale:** `https://100.87.6.45:8443` (remote)

### Monitoring Tools

**System Health:**
```bash
# Agent status across all domains
agent-status

# Service health
clawdbot gateway status
sudo systemctl status crewai-bridge

# Memory system status  
letta status
mcporter list

# Resource monitoring
docker ps              # Media stack containers
htop                    # System resources
```

**Automated Monitoring:**
- **Heartbeats:** Regular agent check-ins (configurable per agent)
- **Auto-status:** Filesystem activity → status summaries
- **Health checks:** Service availability, API endpoints

**Alert Conditions:**
- 🔴 Agent blocked → immediate notify
- Cross-domain conflict → immediate notify  
- Deadline at risk → immediate notify
- No update 48h+ → flag concern

---

## 5. Key Files and Locations

### Agent Workspaces
```
/mnt/ssd/moltbot/
├── clawd/           # Syn (main orchestrator)
├── syl/             # Home/family agent
├── chiron/          # Work/SQL agent  
├── eiron/           # School/MBA agent
└── demiurge/        # Craft/making agent
```

### Core Configuration Files

| File | Purpose | Location |
|------|---------|----------|
| `SOUL.md` | Agent identity/personality | Each workspace |
| `AGENTS.md` | Operating manual | Each workspace |
| `TOOLS.md` | Local tool configurations | Each workspace |
| `MEMORY.md` | Long-term curated memory | Each workspace |
| `BACKLOG.md` | Strategic/someday items | Each workspace |

### Shared Infrastructure
```
/mnt/ssd/moltbot/shared/
├── bin/                    # Shared tools (symlinked)
│   ├── gcal               # Google Calendar
│   ├── tw                 # Taskwarrior wrapper
│   ├── letta              # Memory system
│   ├── memory-router      # Federated memory search
│   ├── bb                 # Blackboard commands
│   ├── task-create        # Task contracts
│   └── facts              # Atomic facts management
├── insights/              # Cross-agent discoveries
├── blackboard/            # Coordination system
├── task-contracts/        # Formal task delegation
└── config/               # Shared configurations
    ├── mcporter.json     # MCP server config
    └── letta-agents.json # Letta agent mapping
```

### Memory Locations
```
<workspace>/memory/
├── YYYY-MM-DD.md         # Daily session logs
├── facts.jsonl           # Atomic facts
├── heartbeat-state.json  # Heartbeat tracking
└── code-audit-*.md       # Quality reports
```

### Domain-Specific Locations

**Syl (Home/Family):**
```
/mnt/ssd/moltbot/syl/
├── family/               # Family-related documents
├── calendar/             # Calendar integrations
└── routines/             # Daily/weekly routines
```

**Chiron (Work):**
```
/mnt/ssd/moltbot/clawd/work/  # Read-only reference
├── projects/             # Project registry  
├── sql/                  # SQL scripts by domain
└── dashboards/           # Dashboard definitions
```

**Eiron (School):**
```
/mnt/ssd/moltbot/clawd/mba/   # MBA materials
├── classes/              # By class (acf, strategic_mgmt, etc.)
├── assignments/          # Homework and projects
└── resources/            # Reference materials
```

**Demiurge (Craft):**
```
/mnt/ssd/moltbot/demiurge/
├── projects/             # Active making projects
├── designs/              # 3D models, patterns
├── materials/            # Inventory and sourcing
└── tools/                # Tool maintenance logs
```

### Configuration Details

**Clawdbot Config (`~/.config/clawdbot/config.json`):**
- Agent bindings to communication channels
- Model configuration (Claude Sonnet-4)
- Tool/skill integrations
- Session management settings

**MCP Config (`/mnt/ssd/moltbot/shared/config/mcporter.json`):**
- MCP server definitions (memory, github, calendar)
- Authentication tokens
- Tool schemas and capabilities

**Letta Config (`~/.config/letta/config.yaml`):**
- Agent definitions and memory stores
- Model configuration for memory operations  
- Archival storage settings

### Bootstrap and Recovery

**New Agent Onboarding:**
1. Clone workspace structure from template
2. Create `BOOTSTRAP.md` with setup instructions  
3. Agent reads bootstrap → configures itself → deletes bootstrap
4. Register with shared infrastructure (Letta, blackboard, etc.)

**Disaster Recovery:**
1. **Memory:** Daily files + MEMORY.md + facts.jsonl backup
2. **Configuration:** Version controlled in git
3. **Shared State:** Blackboard + task-contracts backed up nightly
4. **Service Recovery:** `clawdbot doctor` → restart services

**Common Issues:**
- **Signal group IDs:** Case-sensitive base64, don't lowercase
- **Model names:** Need full date suffix (`-20250514`)  
- **Config validation:** Always run `clawdbot doctor` before restart
- **Session locks:** Use `sessions_list` to find stuck sessions

---

## Design Principles

1. **Specialized Autonomy:** Each agent masters its domain without micromanagement
2. **Shared Infrastructure:** Common tools and coordination systems
3. **Temporal Memory:** Three-tier system balances recency, relevance, and permanence
4. **Graceful Degradation:** System continues working if individual components fail
5. **Human-Readable State:** All system state is inspectable and debuggable
6. **Documented Evolution:** System learns and documents its own improvements

---

*For detailed operational procedures, see individual agent TOOLS.md files and shared infrastructure documentation.*