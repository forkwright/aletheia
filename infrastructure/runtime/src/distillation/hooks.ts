// Distillation hooks — memory flush and plugin notification
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
): Promise<{ flushed: number; errors: number }> {
  const memories: string[] = [
    ...extraction.facts,
    ...extraction.decisions.map((d) => `Decision: ${d}`),
  ];

  if (memories.length === 0) {
    return { flushed: 0, errors: 0 };
  }

  try {
    const result = await target.addMemories(agentId, memories);
    log.info(
      `Memory flush for ${agentId}: ${result.added} added, ${result.errors} errors`,
    );
    return { flushed: result.added, errors: result.errors };
  } catch (err) {
    log.error(
      `Memory flush failed for ${agentId}: ${err instanceof Error ? err.message : err}`,
    );
    return { flushed: 0, errors: memories.length };
  }
}
