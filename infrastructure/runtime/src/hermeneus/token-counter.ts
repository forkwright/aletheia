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
