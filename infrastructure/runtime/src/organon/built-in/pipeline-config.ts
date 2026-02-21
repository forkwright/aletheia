// pipeline_config tool — agents tune their own recall, tool expiry, and note budget
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { clearPipelineConfigCache, loadPipelineConfig, PipelineConfigSchema } from "../../nous/pipeline-config.js";
import type { ToolHandler } from "../registry.js";

export function createPipelineConfigTool(): ToolHandler {
  return {
    definition: {
      name: "pipeline_config",
      description:
        "Read or modify your pipeline configuration (recall thresholds, tool expiry, note budget). " +
        "Changes are saved to your workspace and take effect next turn. " +
        "Use action=read to see current config, action=write with a config object to update, action=reset to restore defaults.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["read", "write", "reset"],
            description: "read: show current config. write: merge provided values. reset: delete config file and use defaults.",
          },
          config: {
            type: "object",
            description: "Partial config to merge (only for action=write). Keys: recall, tools, notes.",
            properties: {
              recall: {
                type: "object",
                properties: {
                  limit: { type: "number", description: "Max memories to recall (1-30, default 8)" },
                  maxTokens: { type: "number", description: "Max tokens for recall block (100-5000, default 1500)" },
                  minScore: { type: "number", description: "Min similarity score (0-1, default 0.75)" },
                  sufficiencyThreshold: { type: "number", description: "Score threshold to skip graph fallback (0-1, default 0.85)" },
                  sufficiencyMinHits: { type: "number", description: "Min hits above sufficiency to skip graph (1-20, default 3)" },
                },
              },
              tools: {
                type: "object",
                properties: {
                  expiryTurns: { type: "number", description: "Turns before unused tools expire (1-50, default 5)" },
                },
              },
              notes: {
                type: "object",
                properties: {
                  tokenCap: { type: "number", description: "Max tokens for agent notes block (100-10000, default 2000)" },
                },
              },
            },
          },
        },
        required: ["action"],
      },
    },
    async execute(input, context) {
      const action = input["action"] as string;
      const filePath = join(context.workspace, "pipeline.json");

      if (action === "read") {
        const config = loadPipelineConfig(context.workspace);
        return JSON.stringify({ config, source: existsSync(filePath) ? "workspace" : "defaults" });
      }

      if (action === "reset") {
        if (existsSync(filePath)) {
          unlinkSync(filePath);
          clearPipelineConfigCache(context.workspace);
        }
        const defaults = PipelineConfigSchema.parse({});
        return JSON.stringify({ config: defaults, source: "defaults", message: "Pipeline config reset to defaults." });
      }

      if (action === "write") {
        const partial = input["config"] as Record<string, unknown> | undefined;
        if (!partial) return JSON.stringify({ error: "config object required for action=write" });

        const existing = loadExistingConfig(filePath);
        const merged = deepMerge(existing, partial);

        const result = PipelineConfigSchema.safeParse(merged);
        if (!result.success) {
          return JSON.stringify({ error: "Validation failed", issues: result.error.issues });
        }

        writeFileSync(filePath, JSON.stringify(result.data, null, 2) + "\n", "utf-8");
        clearPipelineConfigCache(context.workspace);
        return JSON.stringify({ config: result.data, source: "workspace", message: "Pipeline config updated." });
      }

      return JSON.stringify({ error: `Unknown action: ${action}` });
    },
  };
}

function loadExistingConfig(filePath: string): Record<string, unknown> {
  if (!existsSync(filePath)) return {};
  try {
    return JSON.parse(readFileSync(filePath, "utf-8")) as Record<string, unknown>;
  } catch {
    return {};
  }
}

function deepMerge(target: Record<string, unknown>, source: Record<string, unknown>): Record<string, unknown> {
  const result = { ...target };
  for (const key of Object.keys(source)) {
    const sv = source[key];
    const tv = result[key];
    if (sv && typeof sv === "object" && !Array.isArray(sv) && tv && typeof tv === "object" && !Array.isArray(tv)) {
      result[key] = deepMerge(tv as Record<string, unknown>, sv as Record<string, unknown>);
    } else {
      result[key] = sv;
    }
  }
  return result;
}
