// Complexity scoring for adaptive inference routing
// import { createLogger } from "../koina/logger.js";
// const log = createLogger("hermeneus.complexity");

export type ComplexityTier = "routine" | "standard" | "complex";

export interface ComplexityResult {
  tier: ComplexityTier;
  score: number;
  reason: string;
}

const SIMPLE_RESPONSE = /^(yes|no|ok|thanks|sure|got it|hi|hello|hey|yep|nope|k)\b/i;
const COMPLEX_INTENT =
  /\b(analyze|plan|design|implement|debug|review|compare|explain why|architecture|strategy|refactor|investigate|evaluate|diagnose)\b/i;
const TOOL_KEYWORDS =
  /\b(search|find|edit|run|execute|create|delete|deploy|build|test|install|configure|check|read|write)\b/i;
const MULTI_STEP =
  /\b(then|after that|next|also|and then|step \d|first.*then|finally)\b/i;

export function scoreComplexity(opts: {
  messageText: string;
  messageCount: number;
  depth: number;
  agentOverride?: ComplexityTier;
}): ComplexityResult {
  if (opts.agentOverride) {
    const score =
      opts.agentOverride === "complex"
        ? 100
        : opts.agentOverride === "standard"
          ? 50
          : 10;
    return { tier: opts.agentOverride, score, reason: "agent override" };
  }

  if (opts.depth > 0) {
    return { tier: "complex", score: 90, reason: "cross-agent" };
  }

  const text = opts.messageText;
  let score = 30;
  const factors: string[] = [];

  if (text.length < 50) {
    score -= 20;
    factors.push("short");
  } else if (text.length > 500) {
    score += 20;
    factors.push("long");
  }

  if (opts.messageCount === 0) {
    score += 15;
    factors.push("first message");
  }

  if (SIMPLE_RESPONSE.test(text)) {
    score -= 25;
    factors.push("simple response");
  }

  if (COMPLEX_INTENT.test(text)) {
    score += 25;
    factors.push("complex intent");
  }

  if (TOOL_KEYWORDS.test(text)) {
    score = Math.max(score, 40);
    factors.push("tool keywords");
  }

  if (MULTI_STEP.test(text)) {
    score += 15;
    factors.push("multi-step");
  }

  score = Math.max(0, Math.min(100, score));

  let tier: ComplexityTier;
  if (score >= 60) tier = "complex";
  else if (score >= 30) tier = "standard";
  else tier = "routine";

  return { tier, score, reason: factors.join(", ") || "baseline" };
}

export function selectModel(
  tier: ComplexityTier,
  tiers: { routine: string; standard: string; complex: string },
): string {
  return tiers[tier];
}
