// Cross-chunk contradiction detection via LLM pass
import { createLogger } from "../koina/logger.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import { extractJson } from "./extract.js";

const log = createLogger("melete:contradiction-detect");

const CROSS_CHUNK_CONTRADICTION_PROMPT = `You are reviewing a numbered list of facts extracted from a conversation.

Your task: identify pairs of facts that directly contradict each other.

A contradiction means one fact directly negates or conflicts with another — not merely a nuance or update, but an outright logical conflict.

Examples of contradictions:
- "User prefers coffee" vs "User dislikes all hot beverages"
- "Meeting scheduled for Monday" vs "Meeting was cancelled"
- "Server runs on port 8080" vs "Server runs on port 3000"

Examples that are NOT contradictions:
- Two facts about different topics
- A general statement and a specific case
- Two facts that can both be true simultaneously

Return ONLY valid JSON with this exact structure:
{"contradictions": ["description of contradiction 1", "description of contradiction 2"]}

If no contradictions are found, return: {"contradictions": []}

Do not include explanations, markdown, or any text outside the JSON.`;

export async function detectCrossChunkContradictions(
  router: ProviderRouter,
  facts: string[],
  model: string,
): Promise<string[]> {
  if (facts.length < 2) return [];

  try {
    const numberedFacts = facts.map((f, i) => `${i + 1}. ${f}`).join("\n");

    const result = await router.complete({
      model,
      system: CROSS_CHUNK_CONTRADICTION_PROMPT,
      messages: [{ role: "user", content: `Facts to review:\n${numberedFacts}` }],
      maxTokens: 1024,
      temperature: 0,
    });

    const text = result.content
      .filter((b): b is { type: "text"; text: string } => b.type === "text")
      .map((b) => b.text)
      .join("");

    const parsed = extractJson(text);
    if (!parsed) {
      log.warn(`Cross-chunk contradiction detection returned no parseable JSON. Raw: ${text.slice(0, 300)}`);
      return [];
    }

    const contradictions = parsed["contradictions"];
    if (!Array.isArray(contradictions)) {
      log.warn("Cross-chunk contradiction response missing contradictions array");
      return [];
    }

    const valid = contradictions.filter((c): c is string => typeof c === "string");
    if (valid.length > 0) {
      log.info(`Cross-chunk contradiction detection found ${valid.length} contradiction(s)`);
    }

    return valid;
  } catch (error) {
    log.warn(`Cross-chunk contradiction detection error — skipping: ${error instanceof Error ? error.message : error}`);
    return [];
  }
}
