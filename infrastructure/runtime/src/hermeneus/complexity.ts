// Complexity scoring for adaptive inference routing
import { createLogger } from "../koina/logger.js";
const log = createLogger("hermeneus.complexity");

export type ComplexityTier = "routine" | "standard" | "complex";

export interface ComplexityResult {
  tier: ComplexityTier;
  score: number;
  reason: string;
}

// --- Override detection ---

/** User-explicit override commands that bypass scoring. */
const FORCE_COMPLEX = /\b(think hard|deep think|opus|be thorough|take your time)\b/i;
const FORCE_ROUTINE = /\b(quick question|just (tell me|answer)|short answer|quick)\b/i;

// --- Pattern categories ---

const SIMPLE_RESPONSE = /^(yes|no|ok|thanks|sure|got it|hi|hello|hey|yep|nope|k|lgtm|ship it|do it|go|go ahead)\b/i;
const COMPLEX_INTENT =
  /\b(analyze|plan|design|implement|debug|review|compare|explain why|architecture|strategy|refactor|investigate|evaluate|diagnose|decide|tradeoff|synthesize|audit|spec|migrate)\b/i;
const TOOL_KEYWORDS =
  /\b(search|find|edit|run|execute|create|delete|deploy|build|test|install|configure|check|read|write|commit|push|merge|pr)\b/i;
const MULTI_STEP =
  /\b(then|after that|next|also|and then|step \d|first.*then|finally|for each|all of)\b/i;
const CODE_BLOCK = /```[\s\S]*```/;
const QUESTION_WORDS = /^(what|how|why|where|when|who|which|can you|could you|would you)/i;
const PHILOSOPHICAL = /\b(meaning|philosophy|ethics|moral|epistem|ontolog|metaphys|existential|consciousness)\b/i;
const DOMAIN_JUDGMENT = /\b(should I|what do you think|your opinion|recommend|advice|best approach|pros and cons|worth it)\b/i;

export function scoreComplexity(opts: {
  messageText: string;
  messageCount: number;
  depth: number;
  agentOverride?: ComplexityTier;
  sessionHasHistory?: boolean;
}): ComplexityResult {
  // Agent-level override (from config)
  if (opts.agentOverride) {
    const score = opts.agentOverride === "complex" ? 100
      : opts.agentOverride === "standard" ? 50 : 10;
    return { tier: opts.agentOverride, score, reason: "agent override" };
  }

  // Cross-agent calls always get full power
  if (opts.depth > 0) {
    return { tier: "complex", score: 90, reason: "cross-agent" };
  }

  const text = opts.messageText;

  // User-explicit overrides
  if (FORCE_COMPLEX.test(text)) {
    return { tier: "complex", score: 95, reason: "user override: think hard" };
  }
  if (FORCE_ROUTINE.test(text)) {
    return { tier: "routine", score: 5, reason: "user override: quick" };
  }

  let score = 30;
  const factors: string[] = [];

  // --- Length signals ---
  if (text.length < 30) {
    score -= 20;
    factors.push("very short");
  } else if (text.length < 80) {
    score -= 10;
    factors.push("short");
  } else if (text.length > 500) {
    score += 15;
    factors.push("long");
  } else if (text.length > 1000) {
    score += 25;
    factors.push("very long");
  }

  // --- First message in session ---
  if (opts.messageCount === 0) {
    score += 15;
    factors.push("first message");
  }

  // --- Simple response patterns ---
  if (SIMPLE_RESPONSE.test(text)) {
    score -= 30;
    factors.push("simple response");
  }

  // --- Complex intent ---
  if (COMPLEX_INTENT.test(text)) {
    score += 25;
    factors.push("complex intent");
  }

  // --- Domain judgment (needs Opus quality) ---
  if (DOMAIN_JUDGMENT.test(text)) {
    score += 20;
    factors.push("judgment");
  }

  // --- Philosophical / nuanced ---
  if (PHILOSOPHICAL.test(text)) {
    score += 20;
    factors.push("philosophical");
  }

  // --- Tool keywords ---
  if (TOOL_KEYWORDS.test(text)) {
    score = Math.max(score, 35);
    factors.push("tool keywords");
  }

  // --- Multi-step ---
  if (MULTI_STEP.test(text)) {
    score += 15;
    factors.push("multi-step");
  }

  // --- Code blocks ---
  if (CODE_BLOCK.test(text)) {
    score += 10;
    factors.push("code block");
  }

  // --- Questions ---
  if (QUESTION_WORDS.test(text) && text.endsWith("?")) {
    if (text.length < 60) {
      score -= 5;
      factors.push("simple question");
    } else {
      score += 5;
      factors.push("detailed question");
    }
  }

  // --- Multiple sentences suggest more complexity ---
  const sentenceCount = text.split(/[.!?]+/).filter(s => s.trim().length > 10).length;
  if (sentenceCount >= 3) {
    score += 10;
    factors.push(`${sentenceCount} sentences`);
  }

  score = Math.max(0, Math.min(100, score));

  let tier: ComplexityTier;
  if (score >= 60) tier = "complex";
  else if (score >= 30) tier = "standard";
  else tier = "routine";

  if (factors.length > 0) {
    log.debug(`Complexity: score=${score} tier=${tier} factors=[${factors.join(", ")}]`);
  }

  return { tier, score, reason: factors.join(", ") || "baseline" };
}

export function selectModel(
  tier: ComplexityTier,
  tiers: { routine: string; standard: string; complex: string },
): string {
  return tiers[tier];
}

export function selectTemperature(tier: ComplexityTier, hasTools: boolean): number {
  if (hasTools) return 0.3;
  switch (tier) {
    case "routine": return 0.3;
    case "standard": return 0.5;
    case "complex": return 0.7;
  }
}

/**
 * Self-escalation: when a cheaper model realizes it needs more capability.
 * Returns the escalation tier or null if no escalation needed.
 */
export function detectSelfEscalation(responseText: string): ComplexityTier | null {
  const escalationPatterns = [
    /\bI('m| am) not (sure|confident|certain)\b.*\b(should|would|could)\b/i,
    /\bthis (requires|needs) (more|deeper|careful) (thought|analysis|consideration)\b/i,
    /\blet me escalate\b/i,
    /\b(beyond|outside) my (capability|scope|ability)\b/i,
    /\bI('d| would) recommend (asking|consulting|using) (a more capable|opus|a stronger)\b/i,
  ];

  for (const pattern of escalationPatterns) {
    if (pattern.test(responseText)) return "complex";
  }
  return null;
}
