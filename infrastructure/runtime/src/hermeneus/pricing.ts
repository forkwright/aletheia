// Model pricing for cost attribution (per million tokens)
interface ModelPricing {
  inputPerMTok: number;
  outputPerMTok: number;
  cacheReadPerMTok: number;
  cacheWritePerMTok: number;
}

const PRICING: Record<string, ModelPricing> = {
  "claude-opus-4-6": {
    inputPerMTok: 15,
    outputPerMTok: 75,
    cacheReadPerMTok: 1.5,
    cacheWritePerMTok: 18.75,
  },
  "claude-sonnet-4-5-20250929": {
    inputPerMTok: 3,
    outputPerMTok: 15,
    cacheReadPerMTok: 0.3,
    cacheWritePerMTok: 3.75,
  },
  "claude-haiku-4-5-20251001": {
    inputPerMTok: 0.8,
    outputPerMTok: 4,
    cacheReadPerMTok: 0.08,
    cacheWritePerMTok: 1,
  },
};

function resolvePricing(model: string | null): ModelPricing {
  if (model) {
    const exact = PRICING[model];
    if (exact) return exact;
    // Fuzzy match: check if model name contains a known key
    if (model.includes("opus")) return PRICING["claude-opus-4-6"]!;
    if (model.includes("sonnet")) return PRICING["claude-sonnet-4-5-20250929"]!;
    if (model.includes("haiku")) return PRICING["claude-haiku-4-5-20251001"]!;
  }
  // Default to Sonnet pricing for unknown models
  return PRICING["claude-sonnet-4-5-20250929"]!;
}

export function calculateTurnCost(usage: {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  model: string | null;
}): number {
  const p = resolvePricing(usage.model);
  return (
    (usage.inputTokens / 1_000_000) * p.inputPerMTok +
    (usage.outputTokens / 1_000_000) * p.outputPerMTok +
    (usage.cacheReadTokens / 1_000_000) * p.cacheReadPerMTok +
    (usage.cacheWriteTokens / 1_000_000) * p.cacheWritePerMTok
  );
}

export interface CostBreakdown {
  inputCost: number;
  outputCost: number;
  cacheReadCost: number;
  cacheWriteCost: number;
  totalCost: number;
}

export function calculateCostBreakdown(usage: {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  model: string | null;
}): CostBreakdown {
  const p = resolvePricing(usage.model);
  const inputCost = (usage.inputTokens / 1_000_000) * p.inputPerMTok;
  const outputCost = (usage.outputTokens / 1_000_000) * p.outputPerMTok;
  const cacheReadCost = (usage.cacheReadTokens / 1_000_000) * p.cacheReadPerMTok;
  const cacheWriteCost = (usage.cacheWriteTokens / 1_000_000) * p.cacheWritePerMTok;
  return {
    inputCost,
    outputCost,
    cacheReadCost,
    cacheWriteCost,
    totalCost: inputCost + outputCost + cacheReadCost + cacheWriteCost,
  };
}
