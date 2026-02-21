# Spec: Interop & Workflows — A2A, Workflow Engine, IDE Integration

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-21
**Source:** Gap Analysis F-16, F-17, F-20, F-33, F-36

---

## Problem

Aletheia's agents communicate through internal primitives (`sessions_send`, `sessions_ask`, blackboard). They can't talk to external agent systems. Multi-step coordination is handled by cron + event bus — sufficient for current needs but brittle for complex conditional workflows. And there's no IDE integration — development happens via terminal or webchat, not embedded in a code editor.

---

## Design

### Phase 1: Event Bus Hardening (F-33)

Before building on the event bus, harden it.

**Dependency ordering** — handlers declare what they need to run after:

```typescript
eventBus.on("turn:after", myHandler, { after: ["audit-logger"] });
```

Topological sort before dispatch. Circular dependency detection at registration time. Small change (~50 LOC) but prevents subtle ordering bugs as more hooks and workflows register listeners.

### Phase 2: Pub/Sub Hub Pattern (F-36)

Cross-agent broadcast topics for coordination:

```typescript
// Agent subscribes to a topic
hub.subscribe("code-review", nousId);

// When any subscriber publishes, all others receive
hub.publish("code-review", { pr: 78, status: "ready", from: "syn" });
```

Built on the event bus + blackboard. Topics are namespaced, messages are ephemeral (configurable TTL). Agents join/leave dynamically.

**Difference from blackboard:** Blackboard is key-value state. Hub is message-passing. Blackboard persists until TTL. Hub messages are delivered once and discarded (unless a subscriber is offline, in which case they're queued).

### Phase 3: Deterministic Workflow Engine (F-17)

Declarative multi-step workflows with state persistence and conditional routing.

**Definition format** — YAML in `shared/workflows/`:

```yaml
# shared/workflows/pr-review.yaml
name: pr-review
trigger:
  event: tool:called
  filter:
    toolName: sessions_spawn
    role: coder
steps:
  - id: review
    action: spawn
    role: reviewer
    input: "Review the changes from the previous coder task: {{trigger.output}}"
  
  - id: decide
    condition: "{{review.output.confidence}} < 0.8"
    action: spawn
    role: reviewer
    input: "Second review needed — first reviewer was uncertain: {{review.output}}"
  
  - id: notify
    action: message
    to: "{{config.notify}}"
    text: "PR review complete: {{review.output.summary}}"

state:
  persistence: sqlite
  ttl: 24h
```

**Engine** — `WorkflowEngine` reads definitions, registers event bus listeners for triggers, manages state in SQLite. Steps execute sequentially or conditionally. State persists across restarts.

**Primitives:**
- `spawn` — delegate to sub-agent
- `message` — send Signal/webchat message
- `condition` — evaluate expression, skip step if false
- `wait` — pause for human input or timer
- `parallel` — run multiple steps concurrently (uses sessions_dispatch)

### Phase 4: A2A Protocol Support (F-16)

Agent-to-Agent protocol (Google/Linux Foundation standard) for external interop.

**Server side** — expose each nous as an A2A agent card:

```json
{
  "name": "syn",
  "description": "Orchestrator and primary partner",
  "url": "https://aletheia.example.com/a2a/syn",
  "capabilities": ["conversation", "code", "research"],
  "protocols": ["a2a/1.0"]
}
```

Gateway endpoints:
- `GET /.well-known/agent.json` — discovery
- `POST /a2a/:nousId/tasks` — create task
- `GET /a2a/:nousId/tasks/:taskId` — poll status
- `POST /a2a/:nousId/tasks/:taskId/messages` — send message

**Client side** — `a2a_delegate` tool for agents to delegate to external A2A agents:

```typescript
{
  name: "a2a_delegate",
  input: {
    agentUrl: "https://other-system.example.com/a2a/agent",
    task: "Review this code for security issues",
    waitForResult: true,
    timeoutSeconds: 300
  }
}
```

**Mapping** — A2A messages map to/from Aletheia's `InboundMessage`. A2A tasks map to ephemeral sessions.

### Phase 5: ACP / IDE Integration (F-20)

Agent Control Protocol adapter for IDE embedding (VS Code, Cursor, etc.).

**Concept** — ACP defines how an IDE communicates with an agent backend. The adapter translates ACP messages to Aletheia gateway sessions:

- ACP `initialize` → create ephemeral session with IDE context
- ACP `message` → route to agent turn
- ACP `tool_result` → feed tool output back
- Agent responses → stream back via ACP

**Scope** — read-only integration first. The IDE provides context (open files, cursor position, diagnostics). The agent responds with analysis, suggestions, code. No auto-apply initially.

**Implementation** — ACP adapter as a gateway middleware that speaks the ACP protocol on one side and Aletheia sessions on the other. ~300 LOC for the translation layer.

---

## Implementation Order

| Phase | What | Effort | Features |
|-------|------|--------|----------|
| **1** | Event bus dependency ordering | Small | F-33 |
| **2** | Pub/Sub hub pattern | Small | F-36 |
| **3** | Deterministic workflow engine | Medium-Large | F-17 |
| **4** | A2A protocol support | Medium | F-16 |
| **5** | ACP / IDE integration | Medium | F-20 |

---

## Evaluation Items

| Question | What to determine |
|----------|-------------------|
| A2A spec stability | Is v1.0 stable enough? Rate of breaking changes? |
| Workflow complexity | How many workflow types do we actually need? Start with 2-3 concrete workflows before generalizing |
| ACP adoption | Which IDEs support ACP? Is the protocol mature? |
| Hub vs. existing patterns | Does pub/sub add value over blackboard + sessions_send? Test with a real coordination scenario first |

---

## Success Criteria

- Event bus handlers execute in declared dependency order
- Cross-agent broadcast works via pub/sub topics
- At least 2 workflows defined and running (PR review, deployment)
- External agents can discover and delegate tasks to Aletheia nous via A2A
- IDE users can interact with agents without leaving their editor
