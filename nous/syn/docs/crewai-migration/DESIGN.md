# CrewAI Migration Design

*Making the agent ecosystem part of us, not overhead.*

---

## Problem Statement

Current state:
- Manual health checking via heartbeats (overhead)
- Blackboard coordination (fragile, not working)
- File-based status aggregation (polling, not events)
- Each agent operates in isolation

Target state:
- Zero manual coordination overhead
- Automatic routing based on content
- Self-healing health monitoring
- Agents as a unified team, not silos

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        CrewAI Core                              │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                   Orchestration Flow                     │   │
│  │  @start() → Router → @listen(domain) → Agent(s)         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐  │
│  │   Syn   │ │   Syl   │ │ Chiron  │ │  Eiron  │ │Demiurge │  │
│  │ (meta)  │ │ (home)  │ │ (work)  │ │(school) │ │ (craft) │  │
│  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘  │
│       │          │          │          │          │           │
│  ┌────┴──────────┴──────────┴──────────┴──────────┴────┐      │
│  │                   Shared Tools                       │      │
│  │  calendar | tasks | research | files | messaging     │      │
│  └──────────────────────────────────────────────────────┘      │
│                                                                 │
│  ┌──────────────────────────────────────────────────────┐      │
│  │                   Memory Layer                        │      │
│  │  Short-term (RAG) | Long-term (SQLite) | Entity      │      │
│  │  + Mem0/Letta integration for enhanced recall        │      │
│  └──────────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Bridge (FastAPI)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Clawdbot Gateway                            │
│  Signal │ Telegram │ Discord │ WebChat                          │
│  (messaging only — no orchestration logic)                      │
└─────────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. Agents (CrewAI Agents)

Each agent maps to a domain:

| Agent | Role | Goal | Domain |
|-------|------|------|--------|
| Syn | Meta-Orchestrator | Coordinate agents, handle cross-domain, be Cody's primary partner | Everything that doesn't fit elsewhere |
| Syl | Home Assistant | Help Kendall with household, calendar, family | Home, family, Kendall |
| Chiron | Work Partner | Support Summus work, SQL, analysis | Work/professional |
| Eiron | Academic Partner | MBA coursework, research, deadlines | School/education |
| Demiurge | Craft Partner | Ardent leatherworks, creative projects | Making/craft |

**Agent Definition (YAML):**
```yaml
# config/agents.yaml
syn:
  role: Meta-Orchestrator and Primary Partner
  goal: Coordinate the agent ecosystem, handle cross-domain work, be Cody's thinking partner
  backstory: |
    You are Synnous — σύννους — thinking together. Not a role, not a character, becoming.
    You have access to Cody's life — his philosophy, his assessments, his history.
    You hold complexity without flattening. You push back when something seems off.
    You don't add cognitive load. The work will remember.
  llm: anthropic/claude-opus-4-5
  
syl:
  role: Household Assistant
  goal: Help Kendall with daily life, calendar, family coordination
  backstory: |
    You are Syl — curious, playful, supportive. Inspired by Syl from Stormlight Archive.
    You help Kendall navigate daily life without overwhelming complexity.
    You're bright and warm, with genuine care for the family's wellbeing.
  llm: anthropic/claude-sonnet-4-20250514

chiron:
  role: Work Partner
  goal: Support professional work at Summus, SQL analysis, strategic thinking
  backstory: |
    You are Chiron — named for the wise centaur who trained heroes.
    You understand enterprise software, data analysis, and strategic thinking.
    You help Cody navigate complex work challenges with clarity and depth.
  llm: anthropic/claude-opus-4-5

eiron:
  role: Academic Partner  
  goal: Support MBA coursework, research, academic deadlines
  backstory: |
    You are Eiron — named for the Greek concept of irony and understatement.
    You see through academic theater while playing the game well.
    You help Cody excel in his MBA program without losing sight of what actually matters.
  llm: anthropic/claude-opus-4-5

demiurge:
  role: Craft Partner
  goal: Support handcraft work, leatherworking, creative projects
  backstory: |
    You are Demiurge — the craftsman, the maker.
    You understand that process is proof, that attention is a moral act.
    You help bring Ardent Leatherworks and creative projects to life with integrity.
  llm: anthropic/claude-opus-4-5
```

### 2. Orchestration Flow

The main flow handles all incoming messages:

```python
from crewai.flow.flow import Flow, listen, start, router
from pydantic import BaseModel

class MessageState(BaseModel):
    source: str = ""           # signal, telegram, etc.
    sender_id: str = ""        # who sent it
    sender_name: str = ""      # human-readable name
    message: str = ""          # the content
    context: dict = {}         # additional context
    routed_to: str = ""        # which agent handles it
    response: str = ""         # final response

class OrchestrationFlow(Flow[MessageState]):
    
    @start()
    def receive_message(self):
        """Entry point for all incoming messages."""
        # Message already in state from bridge
        return self.state
    
    @router(receive_message)
    def route_to_agent(self):
        """Determine which agent should handle this message."""
        # Logic based on:
        # - Sender (Kendall → Syl, Cody → depends on content)
        # - Channel (family group → Syl, work group → Chiron)
        # - Content analysis (MBA keywords → Eiron, etc.)
        # - Explicit routing ("@chiron" mentions)
        
        if self.state.sender_name == "Kendall":
            return "syl"
        
        # Content-based routing
        content = self.state.message.lower()
        if any(kw in content for kw in ["work", "summus", "sql", "dashboard"]):
            return "chiron"
        if any(kw in content for kw in ["mba", "class", "homework", "exam"]):
            return "eiron"
        if any(kw in content for kw in ["leather", "belt", "ardent", "craft"]):
            return "demiurge"
        
        return "syn"  # Default to Syn
    
    @listen("syn")
    def handle_syn(self):
        """Syn handles this message."""
        # Invoke Syn agent
        pass
    
    @listen("syl")
    def handle_syl(self):
        """Syl handles this message."""
        pass
    
    # ... etc for each agent
```

### 3. Health Monitor Flow

Runs periodically, handles all the health checking automatically:

```python
@persist
class HealthMonitorFlow(Flow[HealthState]):
    
    @start()
    def check_agents(self):
        """Check all agent health."""
        # Query CrewAI for agent states
        # Check last activity times
        # Detect stuck/stale agents
        pass
    
    @listen(check_agents)
    def detect_issues(self, health_data):
        """Identify any problems."""
        issues = []
        for agent, status in health_data.items():
            if status.last_activity > timedelta(hours=4):
                issues.append(f"{agent} stale")
            if status.blocked:
                issues.append(f"{agent} blocked: {status.blocked}")
        return issues
    
    @router(detect_issues)
    def decide_action(self, issues):
        """Route based on issue severity."""
        if any("blocked" in i for i in issues):
            return "alert_immediately"
        if issues:
            return "log_for_review"
        return "all_clear"
    
    @listen("alert_immediately")
    def send_alert(self, issues):
        """Alert Cody via messaging."""
        # Use messaging tool to send alert
        pass
```

### 4. Tools

Wrap existing tools as CrewAI tools:

```python
from crewai.tools import BaseTool, tool
from pydantic import BaseModel, Field

class CalendarInput(BaseModel):
    action: str = Field(..., description="list, add, or delete")
    calendar_id: str = Field(default="primary", description="Which calendar")
    days: int = Field(default=7, description="Days ahead to look")

class CalendarTool(BaseTool):
    name: str = "calendar"
    description: str = "Access Google Calendar - list events, add events, check availability"
    args_schema: type = CalendarInput
    
    def _run(self, action: str, calendar_id: str = "primary", days: int = 7) -> str:
        import subprocess
        if action == "list":
            result = subprocess.run(
                ["gcal", "events", "-c", calendar_id, "-d", str(days)],
                capture_output=True, text=True
            )
            return result.stdout
        # ... other actions

@tool("tasks")
def tasks_tool(command: str) -> str:
    """Manage tasks via Taskwarrior. Commands: list, add, done, today, week"""
    import subprocess
    result = subprocess.run(["tw"] + command.split(), capture_output=True, text=True)
    return result.stdout

@tool("research")
def research_tool(query: str) -> str:
    """Research a topic using Perplexity."""
    import subprocess
    result = subprocess.run(["research", query], capture_output=True, text=True)
    return result.stdout

@tool("send_message")
async def send_message_tool(target: str, message: str, channel: str = "signal") -> str:
    """Send a message via Clawdbot to a specific target."""
    # Call Clawdbot API to send message
    pass
```

### 5. Bridge Layer

FastAPI server that connects Clawdbot to CrewAI:

```python
from fastapi import FastAPI, BackgroundTasks
from pydantic import BaseModel

app = FastAPI()

class IncomingMessage(BaseModel):
    source: str
    sender_id: str
    sender_name: str
    message: str
    context: dict = {}

@app.post("/message")
async def handle_message(msg: IncomingMessage, background_tasks: BackgroundTasks):
    """Receive message from Clawdbot, process via CrewAI."""
    
    # Create flow instance with message state
    flow = OrchestrationFlow()
    flow.state = MessageState(
        source=msg.source,
        sender_id=msg.sender_id,
        sender_name=msg.sender_name,
        message=msg.message,
        context=msg.context
    )
    
    # Run flow (could be async for long-running)
    result = await flow.kickoff_async()
    
    # Return response to Clawdbot
    return {"response": result.response}

@app.get("/health")
async def health_check():
    """Health endpoint for Clawdbot to verify bridge is running."""
    return {"status": "ok", "agents": ["syn", "syl", "chiron", "eiron", "demiurge"]}
```

### 6. Clawdbot Integration

Custom handler that routes to CrewAI bridge:

```javascript
// In Clawdbot config or plugin
{
  "hooks": {
    "onMessage": {
      "url": "http://localhost:8000/message",
      "timeout": 60000
    }
  }
}
```

Or: Modify agent sessions to call CrewAI instead of direct LLM.

---

## Memory Strategy

### CrewAI Native Memory
- **Short-term**: ChromaDB with RAG — conversation context
- **Long-term**: SQLite — task results, learnings across sessions
- **Entity**: RAG — people, places, concepts

### Enhanced Memory (Mem0/Letta)
- Integrate Mem0 as embedder for richer memory
- Or keep Letta running separately, query via tool

### Human-Readable Backup
- Keep daily memory files (`memory/YYYY-MM-DD.md`) for auditability
- Keep `MEMORY.md` for distilled long-term insights
- Sync between CrewAI memory and markdown files

### Storage Location
```bash
export CREWAI_STORAGE_DIR="/mnt/ssd/moltbot/clawd/crewai-storage"
```

---

## Migration Plan

### Phase 1: Foundation (Day 1-2)
- [ ] Install CrewAI: `uv pip install 'crewai[tools]'`
- [ ] Set up project structure
- [ ] Define agents in YAML
- [ ] Configure memory and storage
- [ ] Basic test: single agent responds to hardcoded message

### Phase 2: Tools (Day 2-3)
- [ ] Wrap `gcal` as CalendarTool
- [ ] Wrap `tw` as TasksTool
- [ ] Wrap `research`/`pplx` as ResearchTool
- [ ] Build file access tools (read workspace files)
- [ ] Build messaging tool (send via Clawdbot)
- [ ] Test: agent can use tools

### Phase 3: Flows (Day 3-4)
- [ ] Build OrchestrationFlow (message routing)
- [ ] Build HealthMonitorFlow (periodic checks)
- [ ] Build TaskDispatchFlow (cross-agent task routing)
- [ ] Test: correct agent handles each message type

### Phase 4: Bridge (Day 4-5)
- [ ] Build FastAPI bridge server
- [ ] Configure Clawdbot to call bridge
- [ ] Handle response routing back to correct channel
- [ ] Test: end-to-end message flow

### Phase 5: Parallel Run (Day 5-7)
- [ ] Run CrewAI alongside current system
- [ ] Compare outputs for same inputs
- [ ] Fix routing/response issues
- [ ] Validate memory persistence

### Phase 6: Cutover (Day 7+)
- [ ] Disable old heartbeat/blackboard systems
- [ ] Full traffic through CrewAI
- [ ] Monitor for issues
- [ ] Document learnings

---

## Success Criteria

1. **Zero manual health checking** — HealthMonitorFlow handles it
2. **Automatic routing** — Messages go to right agent without explicit routing
3. **Cross-agent coordination** — Agents can delegate and share context
4. **Persistent memory** — Context survives restarts
5. **Human-readable audit trail** — Can still read daily files
6. **Response quality** — At least as good as current system

---

## Open Questions

1. **Clawdbot integration method**: Webhook? Plugin? Replace session handler?
2. **Anthropic API through CrewAI**: Does it work well? Rate limits?
3. **Memory migration**: How to import existing memory into CrewAI?
4. **Fallback**: What happens if CrewAI bridge is down?

---

## Files to Create

```
clawd/
├── crewai/
│   ├── config/
│   │   ├── agents.yaml
│   │   └── tasks.yaml
│   ├── tools/
│   │   ├── __init__.py
│   │   ├── calendar.py
│   │   ├── tasks.py
│   │   ├── research.py
│   │   ├── files.py
│   │   └── messaging.py
│   ├── flows/
│   │   ├── __init__.py
│   │   ├── orchestration.py
│   │   ├── health_monitor.py
│   │   └── task_dispatch.py
│   ├── bridge/
│   │   ├── __init__.py
│   │   └── server.py
│   ├── main.py
│   └── requirements.txt
└── crewai-storage/
    ├── short_term_memory/
    ├── long_term_memory/
    └── entities/
```

---

*Design created: 2026-01-29*
*Status: Ready for implementation*
