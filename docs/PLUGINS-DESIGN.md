# Plugin System (Design Document)

<!-- TODO: not yet implemented — planned for M5 milestone (prostheke crate, WASM host via wasmtime) -->

This document describes the **planned** plugin system. It is not yet implemented in the Rust binary.

**Current extension mechanism:** Domain packs via the `thesauros` crate. See [ARCHITECTURE.md](ARCHITECTURE.md) for the thesauros entry.

---

The design below is from the TypeScript-era plugin loader (`infrastructure/runtime/src/prostheke/`). The Rust implementation will use WASM (wasmtime) instead of JavaScript, but the lifecycle hook model is expected to carry forward.

## Planned Structure

Plugins hook into the agent lifecycle and register custom tools.

## Structure

```text
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

The built-in memory plugin (`infrastructure/memory/aletheia-memory/`) shows the full API:

- `before_agent_start` - searches KnowledgeStore for relevant memories, injects into context
- `agent_end` - extracts facts from conversation via Claude Haiku
- `memory_search` tool - direct agent access to cross-agent memory
