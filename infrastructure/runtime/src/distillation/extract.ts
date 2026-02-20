// Pass 1: Extract structured facts, decisions, and open items from conversation
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";

const log = createLogger("distillation.extract");

export interface ExtractionResult {
  facts: string[];
  decisions: string[];
  openItems: string[];
  keyEntities: string[];
  contradictions: string[];
}

const EXTRACTION_PROMPT = `You are extracting durable knowledge from a conversation in the Aletheia multi-agent system.

## Rules
- Extract FACTS that would be true tomorrow, next week, next month. Skip ephemeral chatter.
- Extract DECISIONS with their rationale — the "why" matters as much as the "what".
- Extract OPEN ITEMS that require future action — include who owns them if stated.
- Extract KEY ENTITIES: people, projects, tools, services, locations referenced.
- For tool results: extract the factual findings, not the tool invocation details.
- NEVER extract: greetings, acknowledgments, timestamps of the conversation itself, meta-commentary about the conversation.
- Each item: one clear sentence. No duplicates. No hedging language.
- If a fact contradicts a previously known fact, note BOTH versions and flag the contradiction.

## Quality Filters
- Skip facts that are obvious from context (e.g., "The user asked a question")
- Skip facts that are purely temporal ("We discussed this at 3pm")
- Prefer specific over vague: "Honda Passport needs brake pads replaced" over "vehicle maintenance discussed"
- Include confidence: prefix uncertain facts with [UNCERTAIN]

Return ONLY valid JSON:
{
  "facts": ["string"],
  "decisions": ["string"],
  "openItems": ["string"],
  "keyEntities": ["string"],
  "contradictions": ["string"]
}`;

const EMPTY_RESULT: ExtractionResult = {
  facts: [],
  decisions: [],
  openItems: [],
  keyEntities: [],
  contradictions: [],
};

const MAX_CHUNK_TOKENS = 80000; // Leave room for system prompt + output within 200K context

export async function extractFromMessages(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  model: string,
): Promise<ExtractionResult> {
  const totalTokens = messages.reduce(
    (sum, m) => sum + estimateTokens(m.content),
    0,
  );

  // If small enough, extract in one pass
  if (totalTokens <= MAX_CHUNK_TOKENS) {
    return extractChunk(router, messages, model);
  }

  // Split into chunks and extract each, then merge
  const parts = Math.max(2, Math.ceil(totalTokens / MAX_CHUNK_TOKENS));
  const chunks = splitMessagesByTokens(messages, parts);

  log.info(
    `Chunked extraction: ${chunks.length} chunks from ${messages.length} messages (${totalTokens} tokens)`,
  );

  const results: ExtractionResult[] = [];
  for (let i = 0; i < chunks.length; i++) {
    const chunk = chunks[i]!;
    log.info(`Extracting chunk ${i + 1}/${chunks.length} (${chunk.length} messages)`);
    const partial = await extractChunk(router, chunk, model);
    results.push(partial);
  }

  return mergeExtractions(results);
}

async function extractChunk(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  model: string,
): Promise<ExtractionResult> {
  const conversation = messages
    .map((m) => `${m.role}: ${m.content}`)
    .join("\n\n");

  const result = await router.complete({
    model,
    system: EXTRACTION_PROMPT,
    messages: [{ role: "user", content: conversation }],
    maxTokens: 4096,
  });

  const text = result.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");

  const parsed = extractJson(text);
  if (!parsed) {
    log.warn(`Extraction returned no parseable JSON. Raw: ${text.slice(0, 300)}`);
    return { ...EMPTY_RESULT };
  }

  return {
    facts: Array.isArray(parsed["facts"]) ? parsed["facts"] : [],
    decisions: Array.isArray(parsed["decisions"]) ? parsed["decisions"] : [],
    openItems: Array.isArray(parsed["openItems"]) ? parsed["openItems"] : [],
    keyEntities: Array.isArray(parsed["keyEntities"]) ? parsed["keyEntities"] : [],
    contradictions: Array.isArray(parsed["contradictions"]) ? parsed["contradictions"] : [],
  };
}

function splitMessagesByTokens(
  messages: Array<{ role: string; content: string }>,
  parts: number,
): Array<Array<{ role: string; content: string }>> {
  const totalTokens = messages.reduce(
    (sum, m) => sum + estimateTokens(m.content),
    0,
  );
  const targetPerChunk = Math.ceil(totalTokens / parts);
  const chunks: Array<Array<{ role: string; content: string }>> = [];
  let current: Array<{ role: string; content: string }> = [];
  let currentTokens = 0;

  for (const message of messages) {
    const msgTokens = estimateTokens(message.content);
    if (currentTokens + msgTokens > targetPerChunk && current.length > 0) {
      chunks.push(current);
      current = [];
      currentTokens = 0;
    }
    current.push(message);
    currentTokens += msgTokens;
  }
  if (current.length > 0) chunks.push(current);

  return chunks;
}

function mergeExtractions(results: ExtractionResult[]): ExtractionResult {
  const merged: ExtractionResult = {
    facts: [],
    decisions: [],
    openItems: [],
    keyEntities: [],
    contradictions: [],
  };

  for (const r of results) {
    merged.facts.push(...r.facts);
    merged.decisions.push(...r.decisions);
    merged.openItems.push(...r.openItems);
    merged.keyEntities.push(...r.keyEntities);
    merged.contradictions.push(...r.contradictions);
  }

  // Deduplicate by exact match
  merged.facts = [...new Set(merged.facts)];
  merged.decisions = [...new Set(merged.decisions)];
  merged.openItems = [...new Set(merged.openItems)];
  merged.keyEntities = [...new Set(merged.keyEntities)];
  merged.contradictions = [...new Set(merged.contradictions)];

  return merged;
}

/** Extract first balanced JSON object from text, with repair fallbacks */
export function extractJson(raw: string): Record<string, unknown> | null {
  // Strategy 1: Balanced brace extraction + direct parse
  const balanced = findBalancedBraces(raw);
  if (balanced) {
    try {
      return JSON.parse(balanced) as Record<string, unknown>;
    } catch {
      // Strategy 2: Repair common LLM issues then parse
      const repaired = repairJson(balanced);
      try {
        log.debug("JSON parsed after repair (balanced extraction)");
        return JSON.parse(repaired) as Record<string, unknown>;
      } catch {
        // Fall through
      }
    }
  }

  // Strategy 3: Greedy regex fallback (original behavior)
  const match = raw.match(/\{[\s\S]*\}/);
  if (match) {
    try {
      return JSON.parse(match[0]) as Record<string, unknown>;
    } catch {
      const repaired = repairJson(match[0]);
      try {
        log.debug("JSON parsed after repair (greedy regex)");
        return JSON.parse(repaired) as Record<string, unknown>;
      } catch {
        log.warn(`All JSON parse strategies failed. Fragment: ${match[0].slice(0, 200)}`);
      }
    }
  }

  return null;
}

/** Find the first balanced {…} block, respecting string escaping. If truncated, close open braces. */
export function findBalancedBraces(text: string): string | null {
  const start = text.indexOf("{");
  if (start === -1) return null;

  let depth = 0;
  let inString = false;
  let escape = false;

  for (let i = start; i < text.length; i++) {
    const ch = text[i]!;
    if (escape) {
      escape = false;
      continue;
    }
    if (ch === "\\") {
      escape = true;
      continue;
    }
    if (ch === '"') {
      inString = !inString;
      continue;
    }
    if (inString) continue;

    if (ch === "{") depth++;
    if (ch === "}") {
      depth--;
      if (depth === 0) return text.slice(start, i + 1);
    }
  }

  // Truncated output — attempt to close open structures
  if (depth > 0) {
    let fragment = text.slice(start);
    // Close any open array bracket before closing braces
    const lastOpen = fragment.lastIndexOf("[");
    const lastClose = fragment.lastIndexOf("]");
    if (lastOpen > lastClose) fragment += "]";
    for (let d = 0; d < depth; d++) fragment += "}";
    log.debug(`Closed ${depth} unclosed brace(s) in truncated JSON`);
    return fragment;
  }

  return null;
}

/** Repair common LLM JSON malformations */
export function repairJson(json: string): string {
  let s = json;
  // Trailing commas before ] or }
  s = s.replace(/,\s*([\]}])/g, "$1");
  // Single-quoted strings → double-quoted (best-effort)
  s = s.replace(/'([^'\\]*(?:\\.[^'\\]*)*)'/g, '"$1"');
  return s;
}
