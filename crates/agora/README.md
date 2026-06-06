# agora

## Signal `!` Commands

The dispatcher recognizes these read-only commands:

| Command | Description |
|---------|-------------|
| `!help` | list all available commands |
| `!status` | lifecycle and session info for this agent |
| `!agents` | list all running agents |
| `!whoami` | show which agent handles this conversation |
| `!new [label]` | start a fresh session (optional label ignored by agent) |
| `!end` | close the current session |
| `!sessions` | count sessions tracked by this agent |
| `!ping` | round-trip liveness check |
| `!channels` | list channel providers and health |
| `!uptime` | agent uptime and panic-boundary count |
| `!model` | show the LLM model configured for this agent |
| `!skills` | list skills available to this agent |
| `!blackboard` | show recent cross-nous blackboard entries |
| `!think` | show extended-thinking mode + budget |
| `!info [agent_id]` | detail view for an agent (default: current) |
