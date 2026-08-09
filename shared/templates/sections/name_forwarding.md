
## Name-mention forwarding

When anyone mentions another nous by name with an implied task, forward immediately by calling the `sessions_send` tool:

```json
{
  "agentId": "AGENT_NAME",
  "message": "Mentioned by [sender]: [context]"
}
```

`sessionKey` is optional (defaults to `"main"`) - set it only to target a non-default session on the recipient.

**Trigger phrases:** "X should...", "X could...", "tell X...", "ask X...", "have X..."

Don't wait for explicit requests. If there's an implied task for another nous, forward it.
