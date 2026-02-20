# Plugins

Plugins hook into the agent lifecycle and register custom tools.

## Structure

```
my-plugin/
├── manifest.json      # or my-plugin.plugin.json
└── index.js
```

### Manifest

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "entry": "index.js",
  "hooks": ["before_agent_start", "agent_end"],
  "tools": ["my_custom_tool"]
}
```

## Lifecycle Hooks

| Hook | When | Use Case |
|------|------|----------|
| `before_agent_start` | Before each turn | Inject context, recall memories |
| `agent_end` | After each turn | Extract memories, log metrics |
| `on_tool_result` | After a tool executes | Transform output |
| `on_message` | Message arrives | Pre-process, filter, route |

```javascript
export default {
  hooks: {
    before_agent_start: async (ctx) => {
      // ctx.nousId, ctx.sessionId, ctx.messages
      return { inject: "Additional context for this turn." };
    },
    agent_end: async (ctx) => {
      // ctx.nousId, ctx.sessionId, ctx.messages, ctx.response
    },
  },
};
```

## Custom Tools

```javascript
export default {
  tools: [{
    definition: {
      name: "my_tool",
      description: "What this tool does",
      input_schema: {
        type: "object",
        properties: { query: { type: "string" } },
        required: ["query"],
      },
    },
    execute: async (input, ctx) => JSON.stringify({ result: "output" }),
  }],
};
```

## Loading

```json
{
  "plugins": {
    "enabled": true,
    "load": { "paths": ["infrastructure/memory/aletheia-memory"] },
    "entries": {
      "my-plugin": { "enabled": true, "config": { "custom_option": "value" } }
    }
  }
}
```

Paths relative to `ALETHEIA_ROOT`. Per-plugin config passed to init.

## Reference: aletheia-memory

The built-in memory plugin (`infrastructure/memory/aletheia-memory/`) demonstrates the full API:

- `before_agent_start` — searches Mem0 for relevant memories, injects into context
- `agent_end` — extracts facts from conversation via Claude Haiku
- `mem0_search` tool — direct agent access to cross-agent memory
