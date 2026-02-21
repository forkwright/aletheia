// Sub-agent role presets for sessions_spawn — delegates to nous/roles definitions
import { isValidRole, type RoleConfig, type RoleName, ROLES } from "../../nous/roles/index.js";

export interface SubAgentRole {
  model?: string;
  maxTurns: number;
  maxTokenBudget: number;
  systemPromptTemplate: string;
  tools: string[];
}

export const ROLE_NAMES: RoleName[] = ["coder", "reviewer", "researcher", "explorer", "runner"];

function toSubAgentRole(config: RoleConfig): SubAgentRole {
  return {
    model: config.model,
    maxTurns: config.maxTurns,
    maxTokenBudget: config.maxTokenBudget,
    systemPromptTemplate: config.systemPrompt,
    tools: config.tools,
  };
}

export const SUB_AGENT_ROLES: Record<string, SubAgentRole> = Object.fromEntries(
  ROLE_NAMES.map((name) => [name, toSubAgentRole(ROLES[name])]),
);

export function resolveRole(name: string): SubAgentRole | null {
  if (!isValidRole(name)) return null;
  return SUB_AGENT_ROLES[name] ?? null;
}
