# Plugins

Aletheia supports plugins that hook into the agent lifecycle and register custom tools.

## Plugin Structure

A plugin is a directory containing a manifest and a JavaScript entry point:

```
my-plugin/
├── manifest.json          # or my-plugin.plugin.json
└── index.js               # Entry point
```

### Manifest Format

```json
{
  "name": "my-plugin",
  "version": "1.0.0",
  "description": "What this plugin does",
  "entry": "index.js",
  "hooks": ["before_agent_start", "agent_end"],
  "tools": ["my_custom_tool"]
}
```

### Manifest Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Plugin identifier |
| `version` | string | No | Semantic version |
| `description` | string | No | Human-readable description |
| `entry` | string | Yes | Path to JS entry point (relative to plugin dir) |
| `hooks` | string[] | No | Lifecycle hooks this plugin implements |
| `tools` | string[] | No | Tool names this plugin registers |

### Manifest File Names

The loader accepts either:
- `manifest.json`
- `*.plugin.json` (e.g., `my-plugin.plugin.json`)

## Lifecycle Hooks

Plugins can implement these hooks:

| Hook | When | Use Case |
|------|------|----------|
| `before_agent_start` | Before each agent turn | Inject context, recall memories |
| `agent_end` | After each agent turn | Extract memories, log metrics |
| `on_tool_result` | After a tool executes | Transform tool output |
| `on_message` | When a message arrives | Pre-process, filter, route |

### Hook Signature

```javascript
export default {
  hooks: {
    before_agent_start: async (ctx) => {
      // ctx.nousId — agent ID
      // ctx.sessionId — session ID
      // ctx.messages — conversation history
      // Return: { inject?: string } to add context
      return { inject: "Additional context for this turn." };
    },

    agent_end: async (ctx) => {
      // ctx.nousId — agent ID
      // ctx.sessionId — session ID
      // ctx.messages — full conversation including this turn
      // ctx.response — the agent's response text
    },
  },
};
```

## Custom Tools

Plugins can register tools that appear in the agent's tool list:

```javascript
export default {
  tools: [
    {
      definition: {
        name: "my_custom_tool",
        description: "What this tool does",
        input_schema: {
          type: "object",
          properties: {
            query: { type: "string", description: "Search query" },
          },
          required: ["query"],
        },
      },
      execute: async (input, ctx) => {
        // input.query — tool input
        // ctx.nousId — agent ID
        // ctx.sessionId — session ID
        return JSON.stringify({ result: "tool output" });
      },
    },
  ],
};
```

## Loading Plugins

In `aletheia.json`:

```json
{
  "plugins": {
    "enabled": true,
    "load": {
      "paths": [
        "infrastructure/memory/aletheia-memory",
        "path/to/my-plugin"
      ]
    },
    "entries": {
      "my-plugin": {
        "enabled": true,
        "config": {
          "custom_option": "value"
        }
      }
    }
  }
}
```

Paths are relative to `ALETHEIA_ROOT`.

### Per-Plugin Config

The `entries` map lets you enable/disable plugins and pass custom config. The config object is available to the plugin at load time.

## Reference: aletheia-memory Plugin

The built-in memory plugin at `infrastructure/memory/aletheia-memory/` demonstrates the full plugin API:

- **Hooks**: `before_agent_start` (recall), `agent_end` (extract)
- **Tools**: `mem0_search` (cross-agent memory search)
- **Integration**: Calls the Mem0 sidecar at `:8230` for vector + graph search

### What It Does

1. **Before each turn**: Searches Mem0 for memories relevant to the conversation context and injects them as system context
2. **After each turn**: Sends the conversation transcript to Mem0 for automatic fact extraction (uses Claude Haiku)
3. **Tool**: Gives agents direct access to search their memory via the `mem0_search` tool

This is the recommended starting point for understanding plugin development.
