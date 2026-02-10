// Tool registry — register, resolve, filter by policy
import { createLogger } from "../koina/logger.js";
import { truncateToolResult } from "./truncate.js";
import type { ToolDefinition } from "../hermeneus/anthropic.js";

const log = createLogger("organon");

const DEFAULT_MAX_RESULT_TOKENS = 8000;

export interface ToolHandler {
  definition: ToolDefinition;
  execute: (
    input: Record<string, unknown>,
    context: ToolContext,
  ) => Promise<string>;
}

export interface ToolContext {
  nousId: string;
  sessionId: string;
  workspace: string;
}

export class ToolRegistry {
  private tools = new Map<string, ToolHandler>();

  register(handler: ToolHandler): void {
    this.tools.set(handler.definition.name, handler);
    log.debug(`Registered tool: ${handler.definition.name}`);
  }

  get(name: string): ToolHandler | undefined {
    return this.tools.get(name);
  }

  getDefinitions(opts?: {
    allow?: string[];
    deny?: string[];
  }): ToolDefinition[] {
    let tools = Array.from(this.tools.values());

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

  get size(): number {
    return this.tools.size;
  }
}
