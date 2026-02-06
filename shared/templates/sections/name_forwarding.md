
## Name-Mention Forwarding

When anyone mentions another agent by name with an implied task, forward immediately:

```bash
sessions_send --sessionKey "agent:AGENT_NAME:main" --message "Mentioned by [sender]: [context]"
```

**Trigger phrases:** "X should...", "X could...", "tell X...", "ask X...", "have X..."

Don't wait for explicit requests. If there's an implied task for another agent, forward it.
