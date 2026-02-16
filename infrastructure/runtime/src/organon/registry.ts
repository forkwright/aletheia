// Tool registry — register, resolve, filter by policy, dynamic loading with expiry
import { createLogger } from "../koina/logger.js";
import { truncateToolResult } from "./truncate.js";
import type { ToolDefinition } from "../hermeneus/anthropic.js";

const log = createLogger("organon");

const DEFAULT_MAX_RESULT_TOKENS = 8000;
const EXPIRY_TURNS = 5;

export interface ToolHandler {
  definition: ToolDefinition;
  execute: (
    input: Record<string, unknown>,
    context: ToolContext,
  ) => Promise<string>;
  category?: "essential" | "available";
}

export interface ToolContext {
  nousId: string;
  sessionId: string;
  workspace: string;
  allowedRoots?: string[];
  depth?: number;
}

interface ActiveToolEntry {
  sessionId: string;
  lastUsedTurn: number;
}

export class ToolRegistry {
  private tools = new Map<string, ToolHandler>();
  private activeTools = new Map<string, ActiveToolEntry>();

  register(handler: ToolHandler): void {
    const name = handler.definition.name;
    if (this.tools.has(name)) {
      log.warn(`Tool name collision: "${name}" is being overwritten`);
    }
    this.tools.set(name, handler);
    log.debug(`Registered tool: ${name}`);
  }

  get(name: string): ToolHandler | undefined {
    return this.tools.get(name);
  }

  getDefinitions(opts?: {
    allow?: string[];
    deny?: string[];
    sessionId?: string;
  }): ToolDefinition[] {
    let tools = Array.from(this.tools.values());

    // Dynamic loading: filter by category if sessionId provided
    if (opts?.sessionId) {
      tools = tools.filter((t) => {
        if (!t.category || t.category === "essential") return true;
        // Available tools only shown if activated for this session
        const key = `${opts.sessionId}:${t.definition.name}`;
        return this.activeTools.has(key);
      });
    }

    if (opts?.allow?.length) {
      const allowed = new Set(opts.allow);
      tools = tools.filter((t) => allowed.has(t.definition.name));
    }

    if (opts?.deny?.length) {
      const denied = new Set(opts.deny);
      tools = tools.filter((t) => !denied.has(t.definition.name));
    }

    return tools.map((t) => t.definition);
  }

  enableTool(name: string, sessionId: string, turnSeq: number): boolean {
    const handler = this.tools.get(name);
    if (!handler) return false;
    if (!handler.category || handler.category === "essential") return true; // Already available

    const key = `${sessionId}:${name}`;
    this.activeTools.set(key, { sessionId, lastUsedTurn: turnSeq });
    log.info(`Tool ${name} enabled for session ${sessionId}`);
    return true;
  }

  recordToolUse(name: string, sessionId: string, turnSeq: number): void {
    const key = `${sessionId}:${name}`;
    const entry = this.activeTools.get(key);
    if (entry) {
      entry.lastUsedTurn = turnSeq;
    }
  }

  expireUnusedTools(sessionId: string, currentTurn: number): string[] {
    const expired: string[] = [];
    for (const [key, entry] of this.activeTools) {
      if (entry.sessionId !== sessionId) continue;
      if (currentTurn - entry.lastUsedTurn >= EXPIRY_TURNS) {
        expired.push(key.split(":").slice(1).join(":"));
        this.activeTools.delete(key);
      }
    }
    if (expired.length > 0) {
      log.info(`Expired tools for session ${sessionId}: ${expired.join(", ")}`);
    }
    return expired;
  }

  getAvailableToolNames(): string[] {
    return Array.from(this.tools.values())
      .filter((t) => t.category === "available")
      .map((t) => t.definition.name);
  }

  async execute(
    name: string,
    input: Record<string, unknown>,
    context: ToolContext,
  ): Promise<string> {
    const handler = this.tools.get(name);
    if (!handler) {
      return JSON.stringify({ error: `Unknown tool: ${name}` });
    }

    // Let errors propagate — manager.ts catches them and sets isError on tool_result
    const result = await handler.execute(input, context);
    return truncateToolResult(result, {
      maxTokens: DEFAULT_MAX_RESULT_TOKENS,
    });
  }

  hasTools(): boolean {
    return this.tools.size > 0;
  }

  get size(): number {
    return this.tools.size;
  }
}
