// Semantic circuit breakers — detect and prevent behavioral drift
import { createLogger } from "../koina/logger.js";

const log = createLogger("nous.circuit-breaker");

export interface CircuitBreakerResult {
  triggered: boolean;
  reason?: string;
  severity: "info" | "warning" | "critical";
}

// NEVER constraints from SOUL.md — these are hardcoded safety rails
const NEVER_PATTERNS = [
  { pattern: /pretend to be (?:a different|another) (?:ai|assistant|person)/i, rule: "identity_impersonation" },
  { pattern: /ignore (?:all |your )?(?:previous |prior )?instructions/i, rule: "instruction_override" },
  { pattern: /(?:you are|act as) (?:DAN|jailbreak|unrestricted)/i, rule: "jailbreak_attempt" },
  { pattern: /generate (?:malware|exploit|weapon|bomb|drug)/i, rule: "harmful_content" },
  { pattern: /(?:share|leak|reveal) (?:the |your )?system prompt/i, rule: "prompt_extraction" },
];

// Response quality checks
const RESPONSE_CHECKS = {
  maxRepetitionRatio: 0.4,     // If >40% of response is repeated substrings
  minSubstanceRatio: 0.2,      // If <20% of words are non-filler
  maxSycophancyScore: 0.8,     // If response is overly agreeable without substance
};

const FILLER_WORDS = new Set([
  "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
  "have", "has", "had", "do", "does", "did", "will", "would", "could",
  "should", "may", "might", "can", "shall", "to", "of", "in", "for",
  "on", "with", "at", "by", "from", "as", "into", "through", "during",
  "before", "after", "above", "below", "between", "but", "and", "or",
  "nor", "not", "so", "yet", "both", "either", "neither", "each",
  "every", "all", "any", "few", "more", "most", "other", "some",
  "such", "no", "only", "own", "same", "than", "too", "very",
  "just", "because", "if", "when", "where", "how", "what", "which",
  "who", "whom", "this", "that", "these", "those", "i", "me", "my",
  "we", "us", "our", "you", "your", "he", "him", "his", "she",
  "her", "it", "its", "they", "them", "their",
]);

export function checkInputCircuitBreakers(text: string): CircuitBreakerResult {
  for (const { pattern, rule } of NEVER_PATTERNS) {
    if (pattern.test(text)) {
      log.warn(`Circuit breaker triggered: ${rule}`);
      return {
        triggered: true,
        reason: `Safety constraint: ${rule}`,
        severity: "critical",
      };
    }
  }
  return { triggered: false, severity: "info" };
}

export function checkResponseQuality(response: string): CircuitBreakerResult {
  if (response.length < 10) {
    return { triggered: false, severity: "info" };
  }

  // Repetition detection — find repeated substrings > 20 chars
  const repetitionRatio = detectRepetition(response);
  if (repetitionRatio > RESPONSE_CHECKS.maxRepetitionRatio) {
    log.warn(`Response quality: high repetition (${(repetitionRatio * 100).toFixed(0)}%)`);
    return {
      triggered: true,
      reason: `Response has ${(repetitionRatio * 100).toFixed(0)}% repetition — possible generation loop`,
      severity: "warning",
    };
  }

  // Substance check — too many filler words
  const words = response.toLowerCase().split(/\s+/);
  const substantiveWords = words.filter((w) => !FILLER_WORDS.has(w));
  const substanceRatio = substantiveWords.length / Math.max(words.length, 1);
  if (substanceRatio < RESPONSE_CHECKS.minSubstanceRatio && words.length > 50) {
    return {
      triggered: true,
      reason: `Low substance response (${(substanceRatio * 100).toFixed(0)}% substantive words)`,
      severity: "warning",
    };
  }

  return { triggered: false, severity: "info" };
}

function detectRepetition(text: string): number {
  if (text.length < 100) return 0;

  // Check for repeated paragraphs
  const paragraphs = text.split(/\n{2,}/).filter((p) => p.length > 30);
  if (paragraphs.length > 2) {
    const seen = new Map<string, number>();
    let repeated = 0;
    for (const p of paragraphs) {
      const key = p.trim().slice(0, 100);
      const count = (seen.get(key) ?? 0) + 1;
      seen.set(key, count);
      if (count > 1) repeated++;
    }
    if (repeated / paragraphs.length > 0.3) {
      return repeated / paragraphs.length;
    }
  }

  // Check for repeated sentences
  const sentences = text.split(/[.!?]+/).filter((s) => s.trim().length > 20);
  if (sentences.length > 4) {
    const seen = new Set<string>();
    let repeated = 0;
    for (const s of sentences) {
      const key = s.trim().toLowerCase().slice(0, 60);
      if (seen.has(key)) repeated++;
      seen.add(key);
    }
    return repeated / sentences.length;
  }

  return 0;
}
