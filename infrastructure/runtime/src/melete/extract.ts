// Pass 1: Extract structured facts, decisions, and open items from conversation
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";

const log = createLogger("melete.extract");

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
- NEVER extract: greetings, acknowledgments, timestamps of the conversation itself, meta-commentary about the conversation.
- Each item: one clear sentence. No duplicates. No hedging language.
- If a fact contradicts a previously known fact, note BOTH versions and flag the contradiction.

## Context-Dependent Filtering
Detect the conversation type and extract accordingly:

**Tool-heavy turns** — extract CONCLUSIONS and OUTCOMES, not tool invocations.
  - Bad: "User ran grep to search for imports"
  - Good: "The auth module has 6 unused exports that should be removed"

**Discussion turns** — extract OPINIONS, PREFERENCES, and DECISIONS.
  - Bad: "User and agent discussed leather options"
  - Good: "User prefers high-grade polymer for cases due to durability; rejects standard composite for this use case"

**Planning turns** — extract PLANS, TIMELINES, and COMMITMENTS.
  - Bad: "User asked about schedule"
  - Good: "Final project due March 15, needs 3 weeks of work, starting after midterms"

**Debugging turns** — extract ROOT CAUSES and FIXES, not the investigation steps.
  - Bad: "Checked logs, found error, restarted service"
  - Good: "Session convergence bug caused by UNIQUE constraint on archived rows; fixed by reactivating instead of re-creating"

**Correction turns** — extract the CORRECTED fact, flag the old one.
  - Bad: "User said the previous answer was wrong"
  - Good: "CORRECTION: Widget torque is actually 42 Nm, not 50 as stated before"

## Quality Filters
- Skip facts obvious from context (e.g., "The user asked a question")
- Skip purely temporal facts ("We discussed this at 3pm")
- Prefer specific over vague: "Model X device needs firmware update" > "device maintenance discussed"
- Prefix uncertain facts with [UNCERTAIN]
- For CORRECTIONS: include both the wrong and right versions

## Do NOT Extract
- Facts about the conversation itself: "The user asked about X", "The agent mentioned Y" — these describe the conversation, not the world
- Session metadata: session IDs, timestamps, tool calls, command invocations
- Acknowledgments and filler: "Sure", "OK", "Got it", "Understood", "Sounds good"
- Vague capability claims: "Uses Python", "Familiar with Git", "Works with APIs" — no actionable knowledge
- File or path operations: "Checked the config file", "Opened the logs" — ephemeral, not knowledge

## Real Examples from Corpus Audit

BAD (actual noise that was extracted — don't produce these):
- "Uses grep" — tool invocation, not knowledge
- "Familiar with confabulation guards" — too generic, no actionable content
- "Works with a system called Aletheia" — obvious from context
- "Manages agent systems" — vague and implied
- "The user asked about configuration" — meta-commentary
- "Ran git status to check repository" — ephemeral action

GOOD (actual high-value extractions):
- "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions default to user_id='default'"
- "Prefers high-grade polymer for cases; rejects standard composite for durability reasons"
- "CORRECTION: Widget torque is actually 42 Nm, not 50 as stated before"
- "Prosoche dedup window set to 8 hours to reduce alert fatigue from static overdue tasks"
- "Project Alpha deadline is March 2026"
- "memoryTarget interface exists in hooks.ts but was never wired — distillation drops all extracted facts"

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

/**
 * Post-extraction noise filters — catches garbage the prompt doesn't filter.
 * Based on actual noise patterns observed in Qdrant corpus audit (2026-02-21).
 */
const NOISE_PATTERNS = [
  // Generic capability/familiarity statements — never actionable
  /^(Uses|Familiar with|Works with|Has experience|Has access|Knows about)\b/i,
  // Subject-predicate meta-commentary about the user
  /^(The user|User|They|He|She) (is|was|has|had|does|did|can|could|will|would|asked|mentioned|said|wants|wanted|needs|needed)\b/i,
  // Ephemeral tool/action invocations
  /^(Runs?|Ran|Executed|Checked|Opened|Closed|Searched|Grepped|Found|Looked at)\b/i,
  // Meta-commentary about the conversation itself
  /^(Asked about|Discussed|Mentioned|Talked about|Referred to|Agreed to|Noted that)\b/i,
  // Vague participation statements
  /^(Manages|Works on|Involved in|Participates in|Contributes to) (a |an |the |some )/i,
  // Session/system artifacts
  /(session|conversation|chat)\s+(id|started|ended|created)/i,
  // Meta-commentary: facts about the conversation, not the world
  /(the user|the agent|the assistant|I was)\s+(asked|told|said|mentioned|noted|indicated)/i,
  // Tool/function invocations recorded as facts
  /(called|invoked|ran|executed)\s+(tool|function|command|script)\b/i,
  // Acknowledgment phrases with no content
  /^(sure|ok|okay|got it|understood|will do|no problem|sounds good)\b/i,
  // File path operation artifacts
  /^(reading|writing|checking|opening|saving)\s+(file|path|directory)\b/i,
  // Pure hedging with no content (assertion then nothing)
  /^(I think|I believe|I'm not sure|maybe|perhaps|it seems|it appears)\s+(that\s+)?$/i,
  // Timestamp-only facts (no content, just temporal reference)
  /^(on|at|around|approximately)\s+\d{1,2}[:/\d-]\d{1,2}/i,
];

const MIN_ITEM_LENGTH = 15;
const MAX_ITEM_LENGTH = 300;

function toStringArray(arr: unknown[]): string[] {
  return arr.filter((x): x is string => typeof x === "string");
}

/** Filter out noise from extracted items. */
function filterNoise(items: string[]): string[] {
  return items.filter((item) => {
    const trimmed = item.trim();
    if (trimmed.length < MIN_ITEM_LENGTH || trimmed.length > MAX_ITEM_LENGTH) return false;
    if (NOISE_PATTERNS.some((p) => p.test(trimmed))) {
      log.debug(`Filtered noise: "${trimmed.slice(0, 60)}"`);
      return false;
    }
    return true;
  });
}

export async function extractFromMessages(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  model: string,
  opts?: { sidecarUrl?: string; signal?: AbortSignal },
): Promise<ExtractionResult> {
  const totalTokens = messages.reduce(
    (sum, m) => sum + estimateTokens(m.content),
    0,
  );

  // If small enough, extract in one pass (no cross-chunk duplicates possible)
  if (totalTokens <= MAX_CHUNK_TOKENS) {
    return extractChunk(router, messages, model, opts?.signal);
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
    const partial = await extractChunk(router, chunk, model, opts?.signal);
    results.push(partial);
  }

  const merged = mergeExtractions(results);

  // Cross-chunk dedup: near-duplicates emerge when the same fact appears in multiple chunks
  if (opts?.sidecarUrl && chunks.length > 1) {
    merged.facts = await deduplicateFactsViaSidecar(merged.facts, opts.sidecarUrl);
  }

  return merged;
}

export async function deduplicateFactsViaSidecar(
  facts: string[],
  sidecarUrl: string,
  threshold = 0.90,
): Promise<string[]> {
  if (facts.length < 2) return facts;

  try {
    const res = await fetch(`${sidecarUrl}/dedup/batch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ texts: facts, threshold }),
      signal: AbortSignal.timeout(15_000),
    });
    if (!res.ok) {
      log.warn(`Dedup sidecar returned ${res.status} — skipping dedup`);
      return facts;
    }
    const data = await res.json() as { deduplicated: string[]; removed: number };
    if (data.removed > 0) {
      log.info(`Cross-chunk dedup removed ${data.removed} near-duplicate facts`);
    }
    return data.deduplicated;
  } catch (error) {
    log.warn(`Dedup sidecar error — skipping: ${error instanceof Error ? error.message : error}`);
    return facts; // Fail-open: return original facts if dedup unavailable
  }
}

async function extractChunk(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  model: string,
  signal?: AbortSignal,
): Promise<ExtractionResult> {
  const conversation = messages
    .map((m) => `${m.role}: ${m.content}`)
    .join("\n\n");

  const result = await router.complete({
    model,
    system: EXTRACTION_PROMPT,
    messages: [{ role: "user", content: conversation }],
    maxTokens: 4096,
    ...(signal ? { signal } : {}),
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

  const raw = {
    facts: Array.isArray(parsed["facts"]) ? toStringArray(parsed["facts"]) : [],
    decisions: Array.isArray(parsed["decisions"]) ? toStringArray(parsed["decisions"]) : [],
    openItems: Array.isArray(parsed["openItems"]) ? toStringArray(parsed["openItems"]) : [],
    keyEntities: Array.isArray(parsed["keyEntities"]) ? toStringArray(parsed["keyEntities"]) : [],
    contradictions: Array.isArray(parsed["contradictions"]) ? toStringArray(parsed["contradictions"]) : [],
  };

  // Apply post-extraction noise filtering to facts and decisions
  // (openItems, keyEntities, and contradictions are kept as-is)
  const filteredFacts = filterNoise(raw.facts);
  const filteredDecisions = filterNoise(raw.decisions);
  const removed = (raw.facts.length - filteredFacts.length) + (raw.decisions.length - filteredDecisions.length);
  if (removed > 0) {
    log.info(`Post-extraction filter removed ${removed} noise items`);
  }

  return {
    ...raw,
    facts: filteredFacts,
    decisions: filteredDecisions,
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
    } catch { /* balanced JSON parse failed — try repair */
      // Strategy 2: Repair common LLM issues then parse
      const repaired = repairJson(balanced);
      try {
        log.debug("JSON parsed after repair (balanced extraction)");
        return JSON.parse(repaired) as Record<string, unknown>;
      } catch { /* repaired JSON parse failed — fall through to greedy regex */
        // Fall through
      }
    }
  }

  // Strategy 3: Greedy regex fallback (original behavior)
  const match = raw.match(/\{[\s\S]*\}/);
  if (match) {
    try {
      return JSON.parse(match[0]) as Record<string, unknown>;
    } catch { /* greedy match parse failed — try repair */
      const repaired = repairJson(match[0]);
      try {
        log.debug("JSON parsed after repair (greedy regex)");
        return JSON.parse(repaired) as Record<string, unknown>;
      } catch { /* all JSON parse strategies failed */
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
