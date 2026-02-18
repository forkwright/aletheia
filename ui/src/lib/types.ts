export interface Agent {
  id: string;
  name: string;
  emoji?: string | null;
  workspace?: string;
  model?: string;
}

export interface Session {
  id: string;
  nousId: string;
  sessionKey: string;
  messageCount: number;
  lastActivity: string | null;
  updatedAt: string;
  tokenCountEstimate?: number;
  distillationCount?: number;
}

export interface HistoryMessage {
  id: string;
  role: "user" | "assistant" | "tool_result";
  content: string;
  createdAt: string;
  seq: number;
  toolCallId?: string;
  toolName?: string;
}

export interface MediaItem {
  contentType: string;
  data: string;
  filename?: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  toolCalls?: ToolCallState[];
  isStreaming?: boolean;
  media?: MediaItem[];
}

export interface ToolCallState {
  id: string;
  name: string;
  status: "running" | "complete" | "error";
  result?: string;
  durationMs?: number;
}

export type TurnStreamEvent =
  | { type: "turn_start"; sessionId: string; nousId: string }
  | { type: "text_delta"; text: string }
  | { type: "tool_start"; toolName: string; toolId: string }
  | { type: "tool_result"; toolName: string; toolId: string; result: string; isError: boolean; durationMs: number }
  | { type: "turn_complete"; outcome: TurnOutcome }
  | { type: "error"; message: string };

export interface TurnOutcome {
  text: string;
  nousId: string;
  sessionId: string;
  toolCalls: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

export interface MetricsData {
  status: string;
  uptime: number;
  timestamp: string;
  nous: NousMetrics[];
  usage: UsageMetrics;
  cron: CronJob[];
  services: ServiceStatus[];
}

export interface NousMetrics {
  id: string;
  name: string;
  activeSessions: number;
  totalMessages: number;
  lastActivity: string | null;
  tokens: {
    input: number;
    output: number;
    cacheRead: number;
    cacheWrite: number;
    turns: number;
  } | null;
}

export interface UsageMetrics {
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheReadTokens: number;
  totalCacheWriteTokens: number;
  turnCount: number;
  cacheHitRate: number;
}

export interface CronJob {
  id: string;
  cron: string;
  nextRun: string;
  lastRun: string | null;
  agentId?: string;
}

export interface ServiceStatus {
  name: string;
  healthy: boolean;
  message?: string;
}

export interface CostSummary {
  totalCost: number;
  agents: AgentCost[];
}

export interface AgentCost {
  agentId: string;
  totalCost: number;
  cost: number;
  turns: number;
}
