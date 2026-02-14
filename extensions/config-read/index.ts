// config_read — Let agents inspect their own runtime config
import type { AletheiaPluginApi, AletheiaPluginToolContext, AnyAgentTool } from "aletheia/plugin-sdk";

const SENSITIVE_KEYS = new Set([
  "apikey", "api_key", "token", "password", "secret",
  "credentials", "auth", "key",
]);

function redactSensitive(obj: unknown, depth = 0): unknown {
  if (depth > 10) return "[depth limit]";
  if (obj === null || obj === undefined) return obj;
  if (typeof obj === "string") return obj;
  if (typeof obj === "number" || typeof obj === "boolean") return obj;
  if (Array.isArray(obj)) return obj.map((v) => redactSensitive(v, depth + 1));
  if (typeof obj === "object") {
    const result: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
      if (SENSITIVE_KEYS.has(k.toLowerCase())) {
        result[k] = typeof v === "string" ? `${v.slice(0, 4)}...` : "[redacted]";
      } else {
        result[k] = redactSensitive(v, depth + 1);
      }
    }
    return result;
  }
  return String(obj);
}

const ConfigReadSchema = {
  type: "object" as const,
  properties: {
    section: {
      type: "string" as const,
      description: "Config section: agents, tools, bindings, channels, gateway, plugins, compaction, memorySearch, heartbeat. Omit for full config.",
    },
  },
};

function createConfigReadTool(ctx: AletheiaPluginToolContext): AnyAgentTool | null {
  if (!ctx.config) return null;

  return {
    label: "Config Read",
    name: "config_read",
    description:
      "Read your own runtime configuration. Returns the current agent defaults, bindings, tools, channels, and other config sections. Sensitive values (API keys, tokens) are redacted.",
    parameters: ConfigReadSchema as any,
    execute: async (_toolCallId: string, params: any) => {
      const section = params?.section as string | undefined;
      const cfg = ctx.config as Record<string, unknown>;

      let result: unknown;
      if (section) {
        const defaults = (cfg.agents as any)?.defaults;
        const sectionMap: Record<string, () => unknown> = {
          agents: () => cfg.agents,
          tools: () => cfg.tools,
          bindings: () => cfg.bindings,
          channels: () => cfg.channels,
          gateway: () => cfg.gateway,
          plugins: () => cfg.plugins,
          compaction: () => defaults?.compaction,
          memorySearch: () => defaults?.memorySearch,
          heartbeat: () => defaults?.heartbeat,
        };
        const getter = sectionMap[section];
        if (!getter) {
          result = { error: `Unknown section: ${section}`, available: Object.keys(sectionMap) };
        } else {
          result = getter() ?? { error: `Section '${section}' not configured` };
        }
      } else {
        result = cfg;
      }

      const redacted = redactSensitive(result);
      const text = JSON.stringify(redacted, null, 2);
      return {
        content: [{ type: "text", text }],
        details: redacted,
      };
    },
  };
}

const plugin = {
  id: "config-read",
  name: "Config Read",
  description: "Lets agents read their own runtime configuration",
  register(api: AletheiaPluginApi) {
    api.registerTool(createConfigReadTool);
  },
};

export default plugin;
