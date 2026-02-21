// Distillation hooks — memory flush with retry
import { createLogger } from "../koina/logger.js";
import { type PiiScanConfig, scanText } from "../koina/pii.js";
import { trySafe } from "../koina/safe.js";
import type { ExtractionResult } from "./extract.js";

const log = createLogger("distillation:hooks");

export interface MemoryFlushTarget {
  addMemories(
    agentId: string,
    memories: string[],
  ): Promise<{ added: number; errors: number }>;
}

export interface FlushOptions {
  maxRetries?: number;
  piiConfig?: { enabled?: boolean; mode?: string; surfaces?: { memory?: boolean }; allowlist?: string[]; detectors?: string[] };
}

export async function flushToMemory(
  target: MemoryFlushTarget,
  agentId: string,
  extraction: ExtractionResult,
  optsOrRetries: number | FlushOptions = 3,
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
