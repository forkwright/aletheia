// Interaction signal classification — heuristic turn-type detection for competence tracking

export type SignalType =
  | "correction"
  | "approval"
  | "followup"
  | "topic_change"
  | "clarification"
  | "escalation"
  | "neutral";

export interface InteractionSignal {
  sessionId: string;
  nousId: string;
  turnSeq: number;
  signal: SignalType;
  confidence: number;
}

const CORRECTION_PATTERNS = [
  /^no[,.\s!]/i,
  /^actually[,\s]/i,
  /^that'?s\s+(wrong|incorrect|not right)/i,
  /^not\s+what\s+I\s+(meant|wanted|asked)/i,
  /^wrong[.\s!]/i,
  /^incorrect/i,
  /^I\s+said\s+/i,
  /^I\s+meant\s+/i,
];

const APPROVAL_PATTERNS = [
  /^(yes|yeah|yep|yup)[,.\s!]/i,
  /^perfect/i,
  /^exactly/i,
  /^great\s+(work|job|thanks)/i,
  /^thanks?[,.\s!]/i,
  /^good\s+(job|work)/i,
  /^(nice|awesome|excellent)/i,
  /^that'?s\s+(right|correct|perfect)/i,
];

const FOLLOWUP_PATTERNS = [
  /^(and|also)\s+/i,
  /^what\s+about\s+/i,
  /^now\s+/i,
  /^next[,\s]/i,
  /^one\s+more\s+thing/i,
  /^can\s+you\s+also/i,
];

const CLARIFICATION_PATTERNS = [
  /what\s+do\s+you\s+mean/i,
  /can\s+you\s+explain/i,
  /I\s+don'?t\s+understand/i,
  /what\s+does\s+that\s+mean/i,
  /clarify/i,
  /elaborate/i,
];

const ESCALATION_PATTERNS = [
  /urgent/i,
  /emergency/i,
  /asap/i,
  /critical/i,
  /ask\s+(syn|chiron|eiron|demiurge|syl|arbor|akron)/i,
  /talk\s+to\s+(syn|chiron|eiron|demiurge|syl|arbor|akron)/i,
];

function computeWordOverlap(a: string, b: string): number {
  const wordsA = new Set(a.toLowerCase().split(/\s+/).filter((w) => w.length > 2));
  const wordsB = new Set(b.toLowerCase().split(/\s+/).filter((w) => w.length > 2));
  if (wordsA.size === 0 || wordsB.size === 0) return 0;

  let intersection = 0;
  for (const w of wordsA) {
    if (wordsB.has(w)) intersection++;
  }
  return intersection / Math.max(wordsA.size, wordsB.size);
}

export function classifyInteraction(
  userMessage: string,
  previousResponse?: string,
): { signal: SignalType; confidence: number } {
  const text = userMessage.trim();

  for (const pattern of CORRECTION_PATTERNS) {
    if (pattern.test(text)) return { signal: "correction", confidence: 0.8 };
  }

  for (const pattern of APPROVAL_PATTERNS) {
    if (pattern.test(text)) return { signal: "approval", confidence: 0.8 };
  }

  for (const pattern of ESCALATION_PATTERNS) {
    if (pattern.test(text)) return { signal: "escalation", confidence: 0.7 };
  }

  for (const pattern of CLARIFICATION_PATTERNS) {
    if (pattern.test(text)) return { signal: "clarification", confidence: 0.7 };
  }

  for (const pattern of FOLLOWUP_PATTERNS) {
    if (pattern.test(text)) return { signal: "followup", confidence: 0.7 };
  }

  // Topic change detection via word overlap with previous response
  if (previousResponse && previousResponse.length > 20) {
    const overlap = computeWordOverlap(text, previousResponse);
    if (overlap < 0.1) return { signal: "topic_change", confidence: 0.6 };
  }

  // Short questions are often clarifications
  if (text.endsWith("?") && text.length < 100) {
    return { signal: "clarification", confidence: 0.5 };
  }

  return { signal: "neutral", confidence: 0.4 };
}
