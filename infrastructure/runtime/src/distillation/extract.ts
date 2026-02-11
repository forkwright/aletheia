// Pass 1: Extract structured facts, decisions, and open items from conversation
import { createLogger } from "../koina/logger.js";
import type { ProviderRouter } from "../hermeneus/router.js";

const log = createLogger("distillation.extract");

export interface ExtractionResult {
  facts: string[];
  decisions: string[];
  openItems: string[];
  keyEntities: string[];
}

const EXTRACTION_PROMPT = `Analyze this conversation and extract structured information.

Return ONLY valid JSON with these fields:
- facts: Array of factual statements established in the conversation
- decisions: Array of decisions made or agreed upon
- openItems: Array of unresolved questions, pending tasks, or things to follow up on
- keyEntities: Array of important names, projects, tools, or concepts referenced

Be thorough but concise. Each item should be a single clear sentence.`;

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
      return { facts: [], decisions: [], openItems: [], keyEntities: [] };
    }
    return JSON.parse(jsonMatch[0]) as ExtractionResult;
  } catch (err) {
    log.warn(`Extraction JSON parse failed: ${err instanceof Error ? err.message : err}. Raw: ${text.slice(0, 200)}`);
    return { facts: [], decisions: [], openItems: [], keyEntities: [] };
  }
}
