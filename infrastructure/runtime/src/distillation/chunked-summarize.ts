// Multi-stage summarization for large conversations
// Adapted from OpenClaw's compaction.ts patterns (splitMessagesByTokenShare, chunkMessagesByMaxTokens)
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ExtractionResult } from "./extract.js";
import { summarizeMessages } from "./summarize.js";

const log = createLogger("distillation.chunked");

type SimpleMessage = { role: string; content: string };

const MAX_TOOL_RESULT_CHARS = 8000;

export function sanitizeToolResults(messages: SimpleMessage[]): SimpleMessage[] {
  return messages.map((m) => {
    if (!m.content.startsWith("[tool:") && !m.content.startsWith("[tool_result]")) return m;
    if (m.content.length <= MAX_TOOL_RESULT_CHARS) return m;
    return {
      ...m,
      content:
        m.content.slice(0, MAX_TOOL_RESULT_CHARS) +
        "\n... [truncated for summarization]",
    };
  });
}

function splitMessagesByTokenShare(
  messages: SimpleMessage[],
  parts: number,
): SimpleMessage[][] {
  if (messages.length === 0) return [];
  const normalizedParts = Math.min(
    Math.max(1, Math.floor(parts)),
    messages.length,
  );
  if (normalizedParts <= 1) return [messages];

  const totalTokens = messages.reduce(
    (sum, m) => sum + estimateTokens(m.content),
    0,
  );
  const targetTokens = totalTokens / normalizedParts;
  const chunks: SimpleMessage[][] = [];
  let current: SimpleMessage[] = [];
  let currentTokens = 0;

  for (const message of messages) {
    const msgTokens = estimateTokens(message.content);
    if (
      chunks.length < normalizedParts - 1 &&
      current.length > 0 &&
      currentTokens + msgTokens > targetTokens
    ) {
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

export async function summarizeInStages(
  router: ProviderRouter,
  messages: SimpleMessage[],
  extraction: ExtractionResult,
  model: string,
  nousId?: string,
  opts?: { maxChunkTokens?: number; minMessagesForSplit?: number },
): Promise<string> {
  if (messages.length === 0) return "No prior conversation history.";

  const maxChunkTokens = opts?.maxChunkTokens ?? 30000;
  const minMessagesForSplit = opts?.minMessagesForSplit ?? 8;
  const totalTokens = messages.reduce(
    (sum, m) => sum + estimateTokens(m.content),
    0,
  );

  if (messages.length < minMessagesForSplit || totalTokens <= maxChunkTokens) {
    return summarizeMessages(router, messages, extraction, model, nousId);
  }

  const parts = Math.max(2, Math.ceil(totalTokens / maxChunkTokens));
  const chunks = splitMessagesByTokenShare(messages, parts).filter(
    (c) => c.length > 0,
  );

  if (chunks.length <= 1) {
    return summarizeMessages(router, messages, extraction, model, nousId);
  }

  log.info(
    `Multi-stage summarization: ${chunks.length} chunks from ${messages.length} messages (${totalTokens} tokens)`,
  );

  const emptyExtraction: ExtractionResult = {
    facts: [],
    decisions: [],
    openItems: [],
    keyEntities: [],
    contradictions: [],
  };

  const partialSummaries: string[] = [];
  for (let i = 0; i < chunks.length; i++) {
    const ext = i === 0 ? extraction : emptyExtraction;
    const partial = await summarizeMessages(
      router,
      chunks[i]!,
      ext,
      model,
      nousId,
    );
    partialSummaries.push(partial);
  }

  if (partialSummaries.length === 1) return partialSummaries[0]!;

  log.info(`Merging ${partialSummaries.length} partial summaries`);
  const mergeContent = partialSummaries
    .map(
      (s, i) => `--- Part ${i + 1} of ${partialSummaries.length} ---\n${s}`,
    )
    .join("\n\n");

  const mergeResult = await router.complete({
    model,
    system:
      "Merge these partial conversation summaries into a single cohesive summary. " +
      "Preserve all decisions, open items, technical details, and specific facts. " +
      "Remove redundancies. Write in second person. Keep under 500 words.",
    messages: [{ role: "user", content: mergeContent }],
    maxTokens: 2048,
  });

  return mergeResult.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");
}
