// Token estimation with safety margins for budget calculations
const CHARS_PER_TOKEN = 3.5;

// Budget-safe overestimate multiplier (adapted from OpenClaw's SAFETY_MARGIN)
// Used by callers for budget allocation where overestimation prevents context overflow
export const SAFETY_MARGIN = 1.15;

// Per-message overhead: role tag, message framing, separator tokens
const MESSAGE_OVERHEAD_TOKENS = 4;

// Per-tool-definition overhead: JSON schema wrapper, description framing
const TOOL_DEF_OVERHEAD_TOKENS = 200;

export function estimateTokens(text: string): number {
  return Math.ceil(text.length / CHARS_PER_TOKEN);
}

export function estimateTokensSafe(text: string): number {
  return Math.ceil(estimateTokens(text) * SAFETY_MARGIN);
}

export function estimateMessageTokens(
  messages: Array<{ role: string; content: string }>,
): number {
  let total = 0;
  for (const msg of messages) {
    total += estimateTokens(msg.content) + MESSAGE_OVERHEAD_TOKENS;
  }
  return total;
}

export function estimateToolDefTokens(
  toolDefs: unknown[],
): number {
  if (toolDefs.length === 0) return 0;
  const jsonTokens = estimateTokens(JSON.stringify(toolDefs));
  const overhead = toolDefs.length * TOOL_DEF_OVERHEAD_TOKENS;
  return Math.ceil((jsonTokens + overhead) * SAFETY_MARGIN);
}

// Per-tool character limits for stored tool results.
// The model sees full results for its current decision;
// stored results (replayed on future turns) get truncated to these limits.
const TOOL_RESULT_CHAR_LIMITS: Record<string, number> = {
  exec: 8000,
  read: 10000,
  grep: 5000,
  find: 3000,
  ls: 2000,
  web_fetch: 8000,
  web_search: 4000,
  mem0_search: 4000,
  sessions_spawn: 6000,
  sessions_ask: 4000,
};
const DEFAULT_RESULT_CHAR_LIMIT = 5000;

/**
 * Truncate a tool result for storage, preserving head and tail for context.
 * Returns the original string if it's within the limit.
 */
export function truncateToolResult(toolName: string, result: string): string {
  const limit = TOOL_RESULT_CHAR_LIMITS[toolName] ?? DEFAULT_RESULT_CHAR_LIMIT;
  if (result.length <= limit) return result;

  const headSize = Math.floor(limit * 0.7);
  const tailSize = limit - headSize - 80;
  const head = result.slice(0, headSize);
  const tail = result.slice(-tailSize);
  const omitted = result.length - headSize - tailSize;

  return `${head}\n\n[... ${omitted} chars truncated for storage ...]\n\n${tail}`;
}
