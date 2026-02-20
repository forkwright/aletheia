// Sub-agent role definitions — typed specialists for delegated work
import { CODER_PROMPT } from "./prompts/coder.js";
import { REVIEWER_PROMPT } from "./prompts/reviewer.js";
import { RESEARCHER_PROMPT } from "./prompts/researcher.js";
import { EXPLORER_PROMPT } from "./prompts/explorer.js";
import { RUNNER_PROMPT } from "./prompts/runner.js";

export interface RoleConfig {
  model: string;
  systemPrompt: string;
  tools: string[];
  maxTurns: number;
  maxTokenBudget: number;
  description: string;
}

export interface SubAgentResult {
  role: string;
  task: string;
  status: "success" | "partial" | "failed";
  summary: string;
  details: Record<string, unknown>;
  filesChanged?: string[];
  issues?: SubAgentIssue[];
  confidence: number;
}

export interface SubAgentIssue {
  severity: "error" | "warning" | "info";
  location?: string;
  message: string;
  suggestion?: string;
}

export type RoleName = "coder" | "reviewer" | "researcher" | "explorer" | "runner";

export const ROLES: Record<RoleName, RoleConfig> = {
  coder: {
    model: "anthropic/claude-sonnet-4-20250514",
    systemPrompt: CODER_PROMPT,
    tools: ["read", "write", "edit", "exec", "grep", "find", "ls"],
    maxTurns: 15,
    maxTokenBudget: 50_000,
    description: "Write code, make edits, run builds. Mechanical changes with clear specs.",
  },
  reviewer: {
    model: "anthropic/claude-sonnet-4-20250514",
    systemPrompt: REVIEWER_PROMPT,
    tools: ["read", "grep", "find", "exec", "ls"],
    maxTurns: 5,
    maxTokenBudget: 30_000,
    description: "Review code changes. Find bugs, style issues, logic errors.",
  },
  researcher: {
    model: "anthropic/claude-sonnet-4-20250514",
    systemPrompt: RESEARCHER_PROMPT,
    tools: ["web_search", "web_fetch", "read", "exec"],
    maxTurns: 10,
    maxTokenBudget: 40_000,
    description: "Web research, doc reading, API exploration, summarizing findings.",
  },
  explorer: {
    model: "anthropic/claude-haiku-3-5-20241022",
    systemPrompt: EXPLORER_PROMPT,
    tools: ["read", "grep", "find", "ls", "exec"],
    maxTurns: 10,
    maxTokenBudget: 20_000,
    description: "Read-only codebase investigation. Grep, trace, summarize.",
  },
  runner: {
    model: "anthropic/claude-haiku-3-5-20241022",
    systemPrompt: RUNNER_PROMPT,
    tools: ["exec", "read", "ls"],
    maxTurns: 5,
    maxTokenBudget: 15_000,
    description: "Execute commands, run tests, check health, report results.",
  },
};

/**
 * Parse a structured result JSON block from sub-agent response text.
 * Sub-agents are instructed to end with a fenced ```json block.
 * Returns null if no valid JSON block found (graceful degradation).
 */
export function parseStructuredResult(responseText: string): SubAgentResult | null {
  // Match the last ```json ... ``` block in the response
  const jsonBlocks = [...responseText.matchAll(/```json\s*\n([\s\S]*?)\n```/g)];
  if (jsonBlocks.length === 0) return null;

  const lastBlock = jsonBlocks[jsonBlocks.length - 1];
  if (!lastBlock?.[1]) return null;

  try {
    const parsed = JSON.parse(lastBlock[1]) as Record<string, unknown>;

    // Validate required fields
    if (
      typeof parsed["status"] !== "string" ||
      typeof parsed["summary"] !== "string"
    ) {
      return null;
    }

    return {
      role: (parsed["role"] as string) ?? "unknown",
      task: (parsed["task"] as string) ?? "",
      status: parsed["status"] as SubAgentResult["status"],
      summary: parsed["summary"] as string,
      details: (parsed["details"] as Record<string, unknown>) ?? {},
      filesChanged: parsed["filesChanged"] as string[] | undefined,
      issues: parsed["issues"] as SubAgentIssue[] | undefined,
      confidence: (parsed["confidence"] as number) ?? 0.5,
    };
  } catch {
    return null;
  }
}

export function isValidRole(role: string): role is RoleName {
  return role in ROLES;
}
