// Pass 1: Extract structured facts, decisions, and open items from conversation
import { createLogger } from "../koina/logger.js";
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

export async function extractFromMessages(
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

  try {
    const jsonMatch = text.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      log.warn(`Extraction returned no JSON object. Raw response: ${text.slice(0, 200)}`);
      return { facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [] };
    }
    const parsed = JSON.parse(jsonMatch[0]) as Record<string, unknown>;
    return {
      facts: Array.isArray(parsed.facts) ? parsed.facts : [],
      decisions: Array.isArray(parsed.decisions) ? parsed.decisions : [],
      openItems: Array.isArray(parsed.openItems) ? parsed.openItems : [],
      keyEntities: Array.isArray(parsed.keyEntities) ? parsed.keyEntities : [],
      contradictions: Array.isArray(parsed.contradictions) ? parsed.contradictions : [],
    };
  } catch (err) {
    log.warn(`Extraction JSON parse failed: ${err instanceof Error ? err.message : err}. Raw: ${text.slice(0, 200)}`);
    return { facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [] };
  }
}
