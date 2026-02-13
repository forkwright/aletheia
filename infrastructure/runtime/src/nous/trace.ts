// Causal tracing — records provenance for each turn
import { appendFileSync, mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("trace");

export interface ToolCallTrace {
  name: string;
  input: Record<string, unknown>;
  output: string;
  durationMs: number;
  isError: boolean;
  reversibility?: string;
  simulationRequired?: boolean;
}

export interface CrossAgentTrace {
  targetNousId: string;
  message: string;
  response?: string;
  durationMs: number;
  disagreement?: string;
}

export interface TurnTrace {
  sessionId: string;
  nousId: string;
  turnSeq: number;
  timestamp: string;
  model: string;

  // Pre-API context
  bootstrapFiles: string[];
  bootstrapTokens: number;
  degradedServices: string[];

  // Tool execution
  toolCalls: ToolCallTrace[];

  // Cross-agent communication
  crossAgentCalls: CrossAgentTrace[];

  // Post-API outcome
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  totalDurationMs: number;
  responseLength: number;
  toolLoops: number;
}

export class TraceBuilder {
  private trace: TurnTrace;
  private startTime: number;

  constructor(sessionId: string, nousId: string, turnSeq: number, model: string) {
    this.startTime = Date.now();
    this.trace = {
      sessionId,
      nousId,
      turnSeq,
      timestamp: new Date().toISOString(),
      model,
      bootstrapFiles: [],
      bootstrapTokens: 0,
      degradedServices: [],
      toolCalls: [],
      crossAgentCalls: [],
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      totalDurationMs: 0,
      responseLength: 0,
      toolLoops: 0,
    };
  }

  setBootstrap(files: string[], tokens: number): void {
    this.trace.bootstrapFiles = files;
    this.trace.bootstrapTokens = tokens;
  }

  setDegradedServices(services: string[]): void {
    this.trace.degradedServices = services;
  }

  addToolCall(call: ToolCallTrace): void {
    this.trace.toolCalls.push(call);
  }

  addCrossAgentCall(call: CrossAgentTrace): void {
    this.trace.crossAgentCalls.push(call);
  }

  setUsage(input: number, output: number, cacheRead: number, cacheWrite: number): void {
    this.trace.inputTokens += input;
    this.trace.outputTokens += output;
    this.trace.cacheReadTokens += cacheRead;
    this.trace.cacheWriteTokens += cacheWrite;
  }

  setResponseLength(len: number): void {
    this.trace.responseLength = len;
  }

  setToolLoops(count: number): void {
    this.trace.toolLoops = count;
  }

  finalize(): TurnTrace {
    this.trace.totalDurationMs = Date.now() - this.startTime;
    return this.trace;
  }
}

export function persistTrace(trace: TurnTrace, workspace: string): void {
  const tracesDir = join(workspace, "..", "..", "shared", "traces");
  if (!existsSync(tracesDir)) mkdirSync(tracesDir, { recursive: true });

  const filePath = join(tracesDir, `${trace.nousId}.jsonl`);
  appendFileSync(filePath, JSON.stringify(trace) + "\n");
  log.debug(`Trace persisted: ${trace.sessionId} turn ${trace.turnSeq} (${trace.totalDurationMs}ms)`);
}
