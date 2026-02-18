// Tool execution timeout — framework-level safety net for hanging tool calls

export interface ToolTimeoutConfig {
  /** Default timeout for all tools (ms). 0 = no timeout. */
  defaultMs: number;
  /** Per-tool overrides. 0 = no framework timeout (tool handles its own). */
  overrides: Record<string, number>;
}

export const DEFAULT_TOOL_TIMEOUTS: ToolTimeoutConfig = {
  defaultMs: 120_000, // 2 minutes
  overrides: {
    exec: 0,           // exec has its own timeout param
    sessions_ask: 0,   // sessions_ask has its own timeout
    sessions_spawn: 0, // long-running by design
    browser: 180_000,
    web_fetch: 60_000,
    web_search: 60_000,
  },
};

export class ToolTimeoutError extends Error {
  constructor(
    public readonly toolName: string,
    public readonly timeoutMs: number,
  ) {
    super(`Tool "${toolName}" timed out after ${Math.round(timeoutMs / 1000)}s`);
    this.name = "ToolTimeoutError";
  }
}

/** Resolve effective timeout for a tool, merging config overrides over defaults. */
export function resolveTimeout(
  toolName: string,
  config?: Partial<ToolTimeoutConfig>,
): number {
  const overrides = { ...DEFAULT_TOOL_TIMEOUTS.overrides, ...(config?.overrides ?? {}) };
  if (toolName in overrides) return overrides[toolName]!;
  return config?.defaultMs ?? DEFAULT_TOOL_TIMEOUTS.defaultMs;
}

/**
 * Wrap a tool execution function with a framework-level timeout.
 * timeoutMs <= 0 disables the timeout (tool manages its own).
 */
export async function executeWithTimeout(
  fn: () => Promise<string>,
  timeoutMs: number,
  toolName: string,
): Promise<string> {
  if (timeoutMs <= 0) {
    return fn();
  }

  let timer: ReturnType<typeof setTimeout>;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new ToolTimeoutError(toolName, timeoutMs)),
      timeoutMs,
    );
  });

  try {
    const result = await Promise.race([fn(), timeoutPromise]);
    clearTimeout(timer!);
    return result;
  } catch (err) {
    clearTimeout(timer!);
    throw err;
  }
}
