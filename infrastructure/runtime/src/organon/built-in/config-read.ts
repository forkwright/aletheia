// Config inspection tool — agents can read their own scoped config
import type { ToolHandler, ToolContext } from "../registry.js";
import type { AletheiaConfig } from "../../taxis/schema.js";
import { resolveNous } from "../../taxis/loader.js";

export function createConfigReadTool(config?: AletheiaConfig): ToolHandler {
  return {
    definition: {
      name: "config_read",
      description:
        "Read configuration values. Returns your agent config, bindings, cron jobs, or system info.",
      input_schema: {
        type: "object",
        properties: {
          section: {
            type: "string",
            description:
              "Config section: 'agent' (your config), 'agents' (all agent IDs), 'bindings', 'cron', 'gateway', 'plugins'",
          },
        },
        required: ["section"],
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      const section = input.section as string;

      if (!config) {
        return JSON.stringify({ error: "Config not available" });
      }

      switch (section) {
        case "agent": {
          const nous = resolveNous(config, context.nousId);
          if (!nous) return JSON.stringify({ error: `Unknown nous: ${context.nousId}` });
          return JSON.stringify({
            id: nous.id,
            name: nous.name,
            workspace: nous.workspace,
            model: nous.model ?? config.agents.defaults.model,
            tools: nous.tools,
            heartbeat: nous.heartbeat,
          });
        }
        case "agents":
          return JSON.stringify(
            config.agents.list.map((a) => ({
              id: a.id,
              name: a.name ?? a.id,
            })),
          );
        case "bindings":
          return JSON.stringify(
            config.bindings.filter((b) => b.agentId === context.nousId),
          );
        case "cron":
          return JSON.stringify(
            config.cron.jobs.filter(
              (j) => !j.agentId || j.agentId === context.nousId,
            ),
          );
        case "gateway":
          return JSON.stringify({
            port: config.gateway.port,
            bind: config.gateway.bind,
          });
        case "plugins":
          return JSON.stringify({
            enabled: config.plugins.enabled,
            count: Object.keys(config.plugins.entries).length,
            plugins: Object.entries(config.plugins.entries).map(
              ([id, p]) => ({ id, enabled: p.enabled }),
            ),
          });
        default:
          return JSON.stringify({ error: `Unknown section: ${section}` });
      }
    },
  };
}

export const configReadTool = createConfigReadTool();
