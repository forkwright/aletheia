// Pipeline types — composable turn execution stages
import type { AletheiaConfig, NousConfig } from "../../taxis/schema.js";
import type { SessionStore } from "../../mneme/store.js";
import type { ProviderRouter } from "../../hermeneus/router.js";
import type { ToolContext, ToolRegistry } from "../../organon/registry.js";
import type { PluginRegistry } from "../../prostheke/registry.js";
import type { Watchdog } from "../../daemon/watchdog.js";
import type { CompetenceModel } from "../competence.js";
import type { UncertaintyTracker } from "../uncertainty.js";
import type { TraceBuilder } from "../trace.js";
import type {
  MessageParam,
  ToolDefinition,
} from "../../hermeneus/anthropic.js";
import type { ToolCallRecord } from "../../organon/skill-learner.js";
import type { LoopDetector } from "../loop-detector.js";
import type { ApprovalGate, ApprovalMode } from "../../organon/approval.js";
import type { MemoryFlushTarget } from "../../distillation/hooks.js";

// --- Plans ---

export interface PlanStep {
  id: number;
  label: string;
  role: "coder" | "reviewer" | "researcher" | "explorer" | "runner" | "self";
  estimatedCostCents: number;
  parallel?: number[];
  status: "pending" | "approved" | "skipped" | "running" | "done" | "failed";
  result?: string;
}

export interface Plan {
  id: string;
  sessionId: string;
  nousId: string;
  steps: PlanStep[];
  totalEstimatedCostCents: number;
  status: "awaiting_approval" | "executing" | "completed" | "cancelled";
  createdAt: string;
  resolvedAt?: string;
}

export interface MediaAttachment {
  contentType: string;
  data: string;
  filename?: string;
}

export interface InboundMessage {
  text: string;
  nousId?: string;
  sessionKey?: string;
  parentSessionId?: string;
  channel?: string;
  peerId?: string;
  peerKind?: string;
  accountId?: string;
  media?: MediaAttachment[];
  model?: string;
  depth?: number;
  // Thread model (Phase 2): resolved by transport layer before manager
  threadId?: string;
  bindingId?: string;
  lockKey?: string;
  toolFilter?: string[];
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
  error?: string;
}

export type TurnStreamEvent =
  | { type: "turn_start"; sessionId: string; nousId: string; turnId: string }
  | { type: "text_delta"; text: string }
  | { type: "thinking_delta"; text: string }
  | { type: "tool_start"; toolName: string; toolId: string; input?: Record<string, unknown> }
  | { type: "tool_result"; toolName: string; toolId: string; result: string; isError: boolean; durationMs: number; tokenEstimate?: number }
  | { type: "tool_approval_required"; turnId: string; toolName: string; toolId: string; input: unknown; risk: string; reason: string }
  | { type: "tool_approval_resolved"; toolId: string; decision: string }
  | { type: "turn_complete"; outcome: TurnOutcome }
  | { type: "turn_abort"; reason: string }
  | { type: "queue_drained"; count: number }
  | { type: "plan_proposed"; plan: Plan }
  | { type: "plan_step_start"; planId: string; stepId: number }
  | { type: "plan_step_complete"; planId: string; stepId: number; status: "done" | "failed"; result?: string }
  | { type: "plan_complete"; planId: string; status: "completed" | "cancelled" }
  | { type: "error"; message: string };

export type SystemBlock = { type: "text"; text: string; cache_control?: { type: "ephemeral" } };

/** Accumulated state passed between pipeline stages. */
export interface TurnState {
  msg: InboundMessage;
  nousId: string;
  sessionId: string;
  sessionKey: string;
  model: string;
  nous: NousConfig;
  workspace: string;
  temperature?: number;
  seq: number;

  systemPrompt: SystemBlock[];
  messages: MessageParam[];
  toolDefs: ToolDefinition[];
  toolContext: ToolContext;
  trace: TraceBuilder;

  // Execution accumulators
  totalToolCalls: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheReadTokens: number;
  totalCacheWriteTokens: number;
  currentMessages: MessageParam[];
  turnToolCalls: ToolCallRecord[];
  loopDetector: LoopDetector;

  // Set by finalize
  outcome?: TurnOutcome;

  // Abort signal for cooperative cancellation
  abortSignal?: AbortSignal;

  // Turn identifier for approval gates
  turnId?: string;
}

/** Services available to all pipeline stages. */
export interface RuntimeServices {
  config: AletheiaConfig;
  store: SessionStore;
  router: ProviderRouter;
  tools: ToolRegistry;
  plugins?: PluginRegistry;
  watchdog?: Watchdog;
  competence?: CompetenceModel;
  uncertainty?: UncertaintyTracker;
  skillsSection?: string;
  approvalGate?: ApprovalGate;
  approvalMode?: ApprovalMode;
  memoryTarget?: MemoryFlushTarget;
}

/** A pipeline stage that transforms TurnState. Return false to short-circuit. */
export interface PipelineStage {
  name: string;
  execute(state: TurnState, services: RuntimeServices): Promise<TurnState | false>;
}

/** The execute stage streams events and returns final TurnState. */
export interface StreamingStage {
  name: string;
  execute(state: TurnState, services: RuntimeServices): AsyncGenerator<TurnStreamEvent, TurnState>;
}
