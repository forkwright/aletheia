// Parallel tool execution — batch grouping by safety model
import type { ToolUseBlock } from "../hermeneus/anthropic.js";

type ToolParallelism = "always" | "never" | "conditional";

const TOOL_PARALLELISM: Record<string, ToolParallelism> = {
  read: "always",
  grep: "always",
  find: "always",
  ls: "always",
  mem0_search: "always",
  web_search: "always",
  web_fetch: "always",
  brave_search: "always",
  note: "always",
  enable_tool: "always",
  sessions_send: "always",
  sessions_ask: "always",
  sessions_spawn: "always",
  check_calibration: "always",
  what_do_i_know: "always",
  recent_corrections: "always",
  context_check: "always",
  status_report: "always",
  blackboard: "conditional",
  write: "conditional",
  edit: "conditional",
  exec: "never",
  message: "never",
  voice_reply: "never",
};

function extractPath(tool: ToolUseBlock): string | undefined {
  const input = tool.input as Record<string, unknown>;
  if (typeof input["path"] === "string") return input["path"];
  if (typeof input["file"] === "string") return input["file"];
  if (tool.name === "blackboard" && typeof input["key"] === "string") return input["key"];
  return undefined;
}

export function groupForParallelExecution(tools: ToolUseBlock[]): ToolUseBlock[][] {
  if (tools.length <= 1) return tools.length === 1 ? [tools] : [];

  const groups: ToolUseBlock[][] = [];
  let batch: ToolUseBlock[] = [];
  const touchedPaths = new Set<string>();

  for (const tool of tools) {
    const parallelism = TOOL_PARALLELISM[tool.name] ?? "never";

    if (parallelism === "never") {
      if (batch.length > 0) groups.push(batch);
      batch = [];
      touchedPaths.clear();
      groups.push([tool]);
      continue;
    }

    if (parallelism === "conditional") {
      const path = extractPath(tool);
      if (path && touchedPaths.has(path)) {
        if (batch.length > 0) groups.push(batch);
        batch = [];
        touchedPaths.clear();
      }
      if (path) touchedPaths.add(path);
    }

    batch.push(tool);
  }

  if (batch.length > 0) groups.push(batch);
  return groups;
}
