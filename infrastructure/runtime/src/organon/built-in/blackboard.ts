// Cross-agent blackboard — persistent shared state with auto-expiry
import type { ToolContext, ToolHandler } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";

export function createBlackboardTool(store: SessionStore): ToolHandler {
  return {
    definition: {
      name: "blackboard",
      description:
        "Read and write shared state visible to all agents. Entries auto-expire.\n\n" +
        "USE WHEN:\n" +
        "- Sharing findings, status, or coordination signals with other agents\n" +
        "- Checking if another agent has posted relevant context\n" +
        "- Leaving breadcrumbs for future sessions\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Storing long-term facts (use memory/mem0 instead)\n" +
        "- Private session-scoped data (just use your workspace files)\n\n" +
        "TIPS:\n" +
        "- Actions: 'write', 'read', 'list', 'delete'\n" +
        "- Default TTL is 1 hour; use ttl_seconds to customize\n" +
        "- Each agent can only update/delete their own entries\n" +
        "- 'list' shows all active keys across agents",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            description: "Action: 'write', 'read', 'list', 'delete'",
          },
          key: {
            type: "string",
            description: "Blackboard key (required for write/read/delete)",
          },
          value: {
            type: "string",
            description: "Value to write (required for write action)",
          },
          ttl_seconds: {
            type: "number",
            description: "Time-to-live in seconds (default 3600 = 1 hour)",
          },
        },
        required: ["action"],
      },
    },
    execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const action = input["action"] as string;
      const key = input["key"] as string | undefined;
      const value = input["value"] as string | undefined;
      const ttl = (input["ttl_seconds"] as number) || 3600;

      switch (action) {
        case "write": {
          if (!key || !value) {
            return Promise.resolve(JSON.stringify({ error: "key and value required for write" }));
          }
          const id = store.blackboardWrite(key, value, context.nousId, ttl);
          return Promise.resolve(JSON.stringify({ written: true, id, key, ttl_seconds: ttl }));
        }
        case "read": {
          if (!key) {
            return Promise.resolve(JSON.stringify({ error: "key required for read" }));
          }
          const entries = store.blackboardRead(key);
          return Promise.resolve(JSON.stringify({ key, entries }));
        }
        case "list": {
          const keys = store.blackboardList();
          return Promise.resolve(JSON.stringify({ keys }));
        }
        case "delete": {
          if (!key) {
            return Promise.resolve(JSON.stringify({ error: "key required for delete" }));
          }
          const deleted = store.blackboardDelete(key, context.nousId);
          return Promise.resolve(JSON.stringify({ deleted, key }));
        }
        default:
          return Promise.resolve(JSON.stringify({ error: `Unknown action: ${action}. Use write/read/list/delete.` }));
      }
    },
  };
}
