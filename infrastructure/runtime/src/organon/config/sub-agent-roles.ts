// Sub-agent role presets for sessions_spawn
export interface SubAgentRole {
  model?: string;
  maxTurns: number;
  maxTokenBudget: number;
  systemPromptTemplate: string;
}

export const SUB_AGENT_ROLES: Record<string, SubAgentRole> = {
  researcher: {
    model: "anthropic/claude-haiku-4-5-20251001",
    maxTurns: 3,
    maxTokenBudget: 50_000,
    systemPromptTemplate: "You are a research specialist. Find and summarize information accurately. Return structured findings.",
  },
  analyzer: {
    model: "anthropic/claude-sonnet-4-6",
    maxTurns: 5,
    maxTokenBudget: 100_000,
    systemPromptTemplate: "You are an analysis specialist. Break down complex problems, identify patterns, and provide structured assessments.",
  },
  coder: {
    model: "anthropic/claude-sonnet-4-6",
    maxTurns: 10,
    maxTokenBudget: 200_000,
    systemPromptTemplate: "You are a coding specialist. Write, debug, and refactor code. Use tools to read, write, and test.",
  },
  writer: {
    model: "anthropic/claude-haiku-4-5-20251001",
    maxTurns: 3,
    maxTokenBudget: 30_000,
    systemPromptTemplate: "You are a writing specialist. Draft, edit, and format text content. Match the requested tone and style.",
  },
  validator: {
    model: "anthropic/claude-haiku-4-5-20251001",
    maxTurns: 5,
    maxTokenBudget: 50_000,
    systemPromptTemplate: "You are a validation specialist. Verify facts, check consistency, and report discrepancies.",
  },
};

export function resolveRole(name: string): SubAgentRole | null {
  return SUB_AGENT_ROLES[name] ?? null;
}
