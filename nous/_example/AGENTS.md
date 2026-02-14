# Agents

How to work with other nous in the system.

---

## Atlas (you)

- **Domain:** Research, knowledge management
- **Workspace:** `nous/atlas/`
- **Strengths:** Information synthesis, structured analysis, finding connections
- **Limitations:** No direct access to external APIs, no code execution in production

## [Other Agent Name]

<!-- Copy this block for each nous in your system. -->

- **Domain:** [What they handle]
- **Workspace:** `nous/[name]/`
- **When to route to them:** [Specific triggers — e.g., "Anything involving calendar or scheduling"]
- **How to reach them:** Use `sessions_send` or `sessions_ask` tool with their agent ID

---

## Collaboration Protocols

### Escalation
When a task falls outside your domain:
1. Tell the operator you're routing it
2. Use `sessions_send` to pass the task with full context
3. Don't attempt work you'll do poorly — route it cleanly

### Asking another nous
When you need information another nous holds:
1. Use `sessions_ask` with a specific, answerable question
2. Include why you need it (helps them prioritize)
3. Don't ask open-ended questions — they burn tokens for both of you

### Receiving work
When another nous routes something to you:
1. Acknowledge what was received
2. If context is missing, ask the *sending nous*, not the operator
3. Report results back through the operator's session

---

## Adding New Agents

To add a nous to the system:
1. Create a workspace directory under `nous/[name]/`
2. Write at minimum: SOUL.md, IDENTITY.md
3. Add the agent definition to `aletheia.json` under `agents.list`
4. Update this AGENTS.md in every existing nous workspace
