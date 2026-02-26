// Distillation hooks — memory flush with retry
import { createLogger } from "../koina/logger.js";
import { type PiiScanConfig, scanText } from "../koina/pii.js";
import { trySafe } from "../koina/safe.js";
import type { ExtractionResult } from "./extract.js";

const log = createLogger("melete:hooks");

const EVOLUTION_CHECK_CONCURRENCY = 5;

export interface MemoryFlushTarget {
  addMemories(
    agentId: string,
    memories: string[],
    sessionId: string,
  ): Promise<{ added: number; errors: number }>;
}

export interface FlushOptions {
  maxRetries?: number;
  piiConfig?: { enabled?: boolean; mode?: string; surfaces?: { memory?: boolean }; allowlist?: string[]; detectors?: string[] };
  /** Sidecar base URL for evolution pre-check (e.g. http://127.0.0.1:8230). */
  sidecarUrl?: string;
}

/**
 * Run evolution pre-check for each memory against the sidecar /evolution/check endpoint.
 * Returns the filtered list of memories that still need add_batch storage.
 * Memories with action="evolved" are already handled by the evolution endpoint atomically.
 * Fail-open: on any per-memory error, keeps the memory for normal add_batch path.
 */
export async function checkEvolutionBeforeFlush(
  memories: string[],
  sidecarUrl: string,
  agentId: string,
): Promise<string[]> {
  // Process in concurrent batches to avoid overwhelming the sidecar
  const results: Array<{ memory: string; evolved: boolean }> = [];

  for (let i = 0; i < memories.length; i += EVOLUTION_CHECK_CONCURRENCY) {
    const batch = memories.slice(i, i + EVOLUTION_CHECK_CONCURRENCY);
    const settled = await Promise.allSettled(
      batch.map(async (memory) => {
        const res = await fetch(`${sidecarUrl}/evolution/check`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ text: memory, user_id: agentId }),
          signal: AbortSignal.timeout(15_000),
        });
        if (!res.ok) {
          log.warn(`Evolution check returned ${res.status} for memory — keeping for add_batch`);
          return { memory, evolved: false };
        }
        const data = await res.json() as { action?: string };
        return { memory, evolved: data.action === "evolved" };
      }),
    );

    for (let j = 0; j < settled.length; j++) {
      const outcome = settled[j]!;
      const memory = batch[j]!;
      if (outcome.status === "fulfilled") {
        results.push(outcome.value);
      } else {
        // Fail-open: keep memory in add_batch path on error
        log.warn(`Evolution check error for memory — keeping for add_batch: ${outcome.reason instanceof Error ? outcome.reason.message : outcome.reason}`);
        results.push({ memory, evolved: false });
      }
    }
  }

  const evolved = results.filter((r) => r.evolved).length;
  const kept = results.filter((r) => !r.evolved).length;
  const errors = results.length - evolved - kept; // always 0 with fail-open, but for clarity
  log.info(`Evolution check: ${evolved} evolved, ${kept} to flush, ${errors} errors`);

  return results.filter((r) => !r.evolved).map((r) => r.memory);
}

export async function flushToMemory(
  target: MemoryFlushTarget,
  agentId: string,
  extraction: ExtractionResult,
  optsOrRetries: number | FlushOptions = 3,
  sessionId: string,
): Promise<{ flushed: number; errors: number }> {
  const opts: FlushOptions = typeof optsOrRetries === "number"
    ? { maxRetries: optsOrRetries }
    : optsOrRetries;
  const maxRetries = opts.maxRetries ?? 3;

  let memories: string[] = [
    ...extraction.facts,
    ...extraction.decisions.map((d) => `Decision: ${d}`),
  ];

  if (opts.piiConfig?.enabled && opts.piiConfig.surfaces?.memory !== false) {
    const piiCfg: PiiScanConfig = {
      mode: (opts.piiConfig.mode as PiiScanConfig["mode"]) ?? "mask",
      allowlist: opts.piiConfig.allowlist,
      detectors: opts.piiConfig.detectors as PiiScanConfig["detectors"],
    };
    memories = memories.map((m) =>
      trySafe("pii-flush", () => scanText(m, piiCfg), { text: m, matches: [], redacted: 0 }).text,
    );
  }

  if (memories.length === 0) {
    return { flushed: 0, errors: 0 };
  }

  // Evolution pre-check: filter out memories already handled by the evolution endpoint
  if (opts.sidecarUrl) {
    memories = await checkEvolutionBeforeFlush(memories, opts.sidecarUrl, agentId);
    if (memories.length === 0) {
      log.info(`All memories evolved — nothing to flush via add_batch`);
      return { flushed: 0, errors: 0 };
    }
  }

  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      const result = await target.addMemories(agentId, memories, sessionId);
      log.info(
        `Memory flush for ${agentId}: ${result.added} added, ${result.errors} errors` +
          (attempt > 1 ? ` (attempt ${attempt})` : ""),
      );
      return { flushed: result.added, errors: result.errors };
    } catch (error) {
      log.warn(
        `Memory flush attempt ${attempt}/${maxRetries} for ${agentId}: ${error instanceof Error ? error.message : error}`,
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
