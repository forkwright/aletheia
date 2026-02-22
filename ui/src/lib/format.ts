export function formatTokens(n: number): string {
  if (!n) return "0";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return Math.round(n / 1_000) + "k";
  return String(n);
}

export function formatUptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function formatTimeSince(dateStr: string | null): string {
  if (!dateStr) return "never";
  const ms = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(ms / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function formatCost(cost: number): string {
  return `$${cost.toFixed(4)}`;
}

// Per-million-token pricing by model family
// CANONICAL SOURCE: infrastructure/runtime/src/hermeneus/pricing.ts
const MODEL_PRICING: Record<string, { input: number; output: number; cacheRead: number; cacheWrite: number }> = {
  opus:   { input: 15,  output: 75, cacheRead: 1.5,  cacheWrite: 18.75 },
  sonnet: { input: 3,   output: 15, cacheRead: 0.3,  cacheWrite: 3.75 },
  haiku:  { input: 0.8, output: 4,  cacheRead: 0.08, cacheWrite: 1 },
};

function resolvePricing(model?: string) {
  if (model) {
    if (model.includes("opus")) return MODEL_PRICING.opus;
    if (model.includes("haiku")) return MODEL_PRICING.haiku;
  }
  return MODEL_PRICING.sonnet;
}

export function calculateMessageCost(usage: {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  model?: string;
}): number {
  const p = resolvePricing(usage.model);
  return (
    (usage.inputTokens / 1_000_000) * p.input +
    (usage.outputTokens / 1_000_000) * p.output +
    (usage.cacheReadTokens / 1_000_000) * p.cacheRead +
    (usage.cacheWriteTokens / 1_000_000) * p.cacheWrite
  );
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

export function formatTimestamp(isoStr: string): string {
  try {
    const d = new Date(isoStr);
    return d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
  } catch {
    return "";
  }
}
