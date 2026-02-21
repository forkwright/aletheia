// After-turn lightweight fact extraction
// Extracts 0-3 durable facts from each turn for immediate memory storage.
// Uses Haiku for speed (~200ms, ~$0.0001/turn). Non-blocking.

import { createLogger } from "../koina/logger.js";
import type { ProviderRouter } from "../hermeneus/router.js";

const log = createLogger("turn-facts");

/**
 * Noise patterns — facts matching these are filtered before storage.
 * Based on actual garbage observed in the Qdrant corpus audit.
 */
const NOISE_PATTERNS = [
  /^(Uses|Familiar with|Works with|Has experience|Has access|Knows about)\b/i,
  /^(The user|User|They|He|She) (is|was|has|had|does|did|can|could|will|would|asked|mentioned|said|wants|wanted|needs|needed)\b/i,
  /^(Runs?|Ran|Executed|Checked|Opened|Closed|Searched|Grepped|Found)\b/i,
  /^(Asked about|Discussed|Mentioned|Talked about|Referred to)\b/i,
];

const MIN_FACT_LENGTH = 20;
const MAX_FACT_LENGTH = 200;
const MIN_RESPONSE_LENGTH = 150; // Skip very short responses

const EXTRACTION_PROMPT = `Extract 0-3 durable facts from this assistant turn. A durable fact is something that would still be true and useful to know next week.

EXTRACT:
- Decisions made (with rationale)
- Preferences stated or confirmed
- Corrections to prior beliefs
- New factual knowledge learned
- Commitments or plans established

DO NOT EXTRACT:
- What tools were used or commands run
- Greetings, acknowledgments, meta-commentary
- Transient states ("server is down", "test failed")
- Obvious context ("user is using Aletheia")
- Generic observations ("user is technical")

QUALITY:
- Each fact must stand alone without conversation context
- Be specific: "Pitman arm torque is 185 ft-lbs" not "torque spec discussed"
- One fact per line, no numbering
- If nothing worth extracting, return empty array

Return ONLY a JSON array of strings: ["fact one", "fact two"]
Return [] if nothing durable.`;

export interface TurnFactsResult {
  facts: string[];
  model: string;
  durationMs: number;
}

/**
 * Extract durable facts from a single turn's assistant response.
 * Returns 0-3 facts suitable for immediate memory storage.
 */
export async function extractTurnFacts(
  router: ProviderRouter,
  assistantText: string,
  toolSummary: string,
  model: string,
): Promise<TurnFactsResult> {
  const start = Date.now();

  // Skip short or trivial responses
  if (assistantText.length < MIN_RESPONSE_LENGTH) {
    return { facts: [], model, durationMs: Date.now() - start };
  }

  // Build a compact input — just the response and tool outcomes, not the full conversation
  const input = toolSummary
    ? `## Tool Results\n${toolSummary.slice(0, 1000)}\n\n## Response\n${assistantText.slice(0, 3000)}`
    : assistantText.slice(0, 3000);

  try {
    const result = await router.complete({
      model,
      system: EXTRACTION_PROMPT,
      messages: [{ role: "user", content: input }],
      maxTokens: 400,
    });

    const text = result.content
      .filter((b): b is { type: "text"; text: string } => b.type === "text")
      .map((b) => b.text)
      .join("");

    const parsed = parseFactsArray(text);
    const filtered = parsed
      .map((f) => f.trim())
      .filter((f) => f.length >= MIN_FACT_LENGTH && f.length <= MAX_FACT_LENGTH)
      .filter((f) => !NOISE_PATTERNS.some((p) => p.test(f)))
      .slice(0, 3);

    const ms = Date.now() - start;
    if (filtered.length > 0) {
      log.debug(`Extracted ${filtered.length} turn facts (${ms}ms): ${filtered[0]?.slice(0, 60)}...`);
    }
    return { facts: filtered, model, durationMs: ms };
  } catch (err) {
    log.debug(`Turn fact extraction failed: ${err instanceof Error ? err.message : err}`);
    return { facts: [], model, durationMs: Date.now() - start };
  }
}

/** Parse a JSON array of strings from LLM output. Tolerant of markdown fences. */
function parseFactsArray(raw: string): string[] {
  // Strip markdown code fences
  let text = raw.trim();
  text = text.replace(/^```(?:json)?\n?/i, "").replace(/\n?```$/i, "");
  text = text.trim();

  try {
    const parsed = JSON.parse(text);
    if (Array.isArray(parsed)) {
      return parsed.filter((item): item is string => typeof item === "string");
    }
  } catch {
    // Try extracting array from within text
    const match = text.match(/\[[\s\S]*\]/);
    if (match) {
      try {
        const arr = JSON.parse(match[0]);
        if (Array.isArray(arr)) {
          return arr.filter((item): item is string => typeof item === "string");
        }
      } catch {
        // Give up
      }
    }
  }
  return [];
}
