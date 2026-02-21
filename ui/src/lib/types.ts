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
  thinking?: string;
  turnOutcome?: TurnOutcome;
}

export interface ToolCallState {
  id: string;
  name: string;
  status: "running" | "complete" | "error";
  input?: Record<string, unknown>;
  result?: string;
  durationMs?: number;
  tokenEstimate?: number;
}

export interface PlanStep {
  id: number;
  label: string;
  role: "coder" | "reviewer" | "researcher" | "explorer" | "runner" | "self";
  estimatedCostCents: number;
  parallel?: number[];
  status: "pending" | "approved" | "skipped" | "running" | "done" | "failed";
  result?: string;
}

export interface PlanProposal {
  id: string;
  sessionId: string;
  nousId: string;
  steps: PlanStep[];
  totalEstimatedCostCents: number;
  status: "awaiting_approval" | "executing" | "completed" | "cancelled";
  createdAt: string;
  goal?: string;
}

export type TurnStreamEvent =
  | { type: "turn_start"; sessionId: string; nousId: string; turnId?: string }
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_start"; toolName: string; toolId: string; input?: Record<string, unknown> }
  | { type: "tool_result"; toolName: string; toolId: string; result: string; isError: boolean; durationMs: number; tokenEstimate?: number }
  | { type: "tool_approval_required"; turnId: string; toolName: string; toolId: string; input: unknown; risk: string; reason: string }
  | { type: "tool_approval_resolved"; toolId: string; decision: string }
  | { type: "plan_proposed"; plan: PlanProposal }
  | { type: "plan_step_start"; planId: string; stepId: number }
  | { type: "plan_step_complete"; planId: string; stepId: number; status: "done" | "failed"; result?: string }
  | { type: "plan_complete"; planId: string; status: "completed" | "cancelled" }
  | { type: "turn_complete"; outcome: TurnOutcome }
  | { type: "turn_abort"; reason: string }
  | { type: "error"; message: string };

export interface PendingApproval {
  turnId: string;
  toolName: string;
  toolId: string;
  input: unknown;
  risk: string;
  reason: string;
}

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

export interface GraphNode {
  id: string;
  labels: string[];
  pagerank: number;
  community: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  rel_type: string;
}

export interface CommunityMeta {
  id: number;
  size: number;
  centroid_node: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
  communities: number;
  community_meta: CommunityMeta[];
  total_nodes: number;
}

export interface EntityRelationship {
  type: string;
  target: string;
  direction: "out" | "in";
}

export interface EntityMemory {
  text: string;
  score: number;
}

export interface EntityDetail {
  name: string;
  properties: Record<string, unknown>;
  relationships: EntityRelationship[];
  memories: EntityMemory[];
  confidence: "high" | "medium" | "low";
  relationship_count: number;
  pagerank: number;
}

export interface CommandInfo {
  name: string;
  description: string;
  aliases: string[];
}

export interface FileTreeEntry {
  name: string;
  type: "file" | "directory";
  size?: number;
  modified?: string;
  children?: FileTreeEntry[];
}

export interface GitFileStatus {
  status: string;
  path: string;
}

export interface Thread {
  id: string;
  nousId: string;
  identity: string;
  createdAt: string;
  updatedAt: string;
  sessionCount: number;
  messageCount: number;
  lastActivity: string | null;
  summary: string | null;
}

export interface ThreadMessage extends HistoryMessage {
  sessionId: string;
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
