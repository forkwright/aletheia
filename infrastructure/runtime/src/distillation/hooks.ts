// Distillation hooks — memory flush with retry
import { createLogger } from "../koina/logger.js";
import type { ExtractionResult } from "./extract.js";

const log = createLogger("distillation:hooks");

export interface MemoryFlushTarget {
  addMemories(
    agentId: string,
    memories: string[],
  ): Promise<{ added: number; errors: number }>;
}

export async function flushToMemory(
  target: MemoryFlushTarget,
  agentId: string,
  extraction: ExtractionResult,
  maxRetries = 3,
): Promise<{ flushed: number; errors: number }> {
  const memories: string[] = [
    ...extraction.facts,
    ...extraction.decisions.map((d) => `Decision: ${d}`),
  ];

  if (memories.length === 0) {
    return { flushed: 0, errors: 0 };
  }

  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const result = await target.addMemories(agentId, memories);
      log.info(
        `Memory flush for ${agentId}: ${result.added} added, ${result.errors} errors` +
          (attempt > 1 ? ` (attempt ${attempt})` : ""),
      );
      return { flushed: result.added, errors: result.errors };
    } catch (err) {
      log.warn(
        `Memory flush attempt ${attempt}/${maxRetries} for ${agentId}: ${err instanceof Error ? err.message : err}`,
      );
      if (attempt < maxRetries) {
        const delay = Math.min(1000 * 2 ** (attempt - 1), 5000);
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  }

  log.error(
    `Memory flush exhausted ${maxRetries} retries for ${agentId} (${memories.length} memories lost)`,
  );
  return { flushed: 0, errors: memories.length };
}
