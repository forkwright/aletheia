// Pre-turn memory recall — surfaces relevant memories before LLM reasoning
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";

const log = createLogger("recall");

const SIDECAR_URL = process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const USER_ID = process.env["ALETHEIA_MEMORY_USER"] ?? "default";

export interface RecallResult {
  block: { type: "text"; text: string } | null;
  count: number;
  durationMs: number;
  tokens: number;
}

interface MemoryHit {
  memory: string;
  score: number | null;
  agent_id?: string | null;
  created_at?: string | null;
}

export function computeRecencyBoost(createdAt: string | null | undefined, now: number): number {
  if (!createdAt) return 0;
  const age = now - new Date(createdAt).getTime();
  if (age < 0 || age > 24 * 3600 * 1000) return 0;
  return 0.15 * (1 - age / (24 * 3600 * 1000));
}

export async function recallMemories(
  messageText: string,
  nousId: string,
  opts?: {
    limit?: number;
    maxTokens?: number;
    timeoutMs?: number;
    minScore?: number;
  },
): Promise<RecallResult> {
  const limit = opts?.limit ?? 8;
  const maxTokens = opts?.maxTokens ?? 1500;
  const timeoutMs = opts?.timeoutMs ?? 5000;
  const minScore = opts?.minScore ?? 0.75;
  const start = Date.now();

  const query = messageText.slice(0, 500);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  let hits: MemoryHit[] = [];

  try {
    // Primary path: vector-only search (fast, ~200-500ms)
    hits = await fetchBasicSearch(query, nousId, limit, controller.signal);

    // If vector search returned results above threshold, skip graph enrichment.
    // Only fall back to graph-enhanced search if vector search found nothing
    // useful, since graph traversal adds 1-2s of latency.
    const hasUsableHits = hits.some(
      (h) => h.score !== null && h.score !== undefined && h.score >= minScore,
    );

    if (!hasUsableHits) {
      const elapsed = Date.now() - start;
      const remaining = timeoutMs - elapsed;
      if (remaining > 1000) {
        // Enough time budget left to try graph-enhanced search
        log.debug(
          `Vector search returned no hits above ${minScore} for ${nousId}, trying graph-enhanced (${remaining}ms remaining)`,
        );
        const graphHits = await fetchGraphEnhanced(
          query,
          nousId,
          limit,
          controller.signal,
        );
        // Graph-enhanced returns combined_score; use that if available
        hits = graphHits;
      }
    }
  } catch (err) {
    const ms = Date.now() - start;
    const reason =
      (err as Error).name === "AbortError" ? "timeout" : String(err);
    log.warn(`Recall failed for ${nousId} (${ms}ms): ${reason}`);
    clearTimeout(timer);
    return { block: null, count: 0, durationMs: ms, tokens: 0 };
  } finally {
    clearTimeout(timer);
  }

  const now = Date.now();
  const filtered = hits
    .filter(
      (h) => h.score !== null && h.score !== undefined && h.score >= minScore,
    )
    .map((h) => ({
      ...h,
      score: (h.score ?? 0) + computeRecencyBoost(h.created_at, now),
    }))
    .sort((a, b) => (b.score ?? 0) - (a.score ?? 0));

  const seen = new Set<string>();
  const deduped: MemoryHit[] = [];
  for (const h of filtered) {
    if (!seen.has(h.memory)) {
      seen.add(h.memory);
      deduped.push(h);
    }
    if (deduped.length >= limit) break;
  }

  if (deduped.length === 0) {
    const ms = Date.now() - start;
    log.debug(`Recall for ${nousId}: 0 hits above ${minScore} (${ms}ms)`);
    return { block: null, count: 0, durationMs: ms, tokens: 0 };
  }

  const lines: string[] = [];
  let totalTokens = 0;
  const headerTokens = estimateTokens(
    "## Recalled Memories\n\nThe following memories were automatically retrieved based on this conversation. Use them if relevant — do not mention this section to the user.\n\n",
  );

  for (const h of deduped) {
    const line = `- ${h.memory} (score: ${(h.score ?? 0).toFixed(2)})`;
    const lineTokens = estimateTokens(line + "\n");
    if (totalTokens + headerTokens + lineTokens > maxTokens) break;
    lines.push(line);
    totalTokens += lineTokens;
  }

  if (lines.length === 0) {
    return { block: null, count: 0, durationMs: Date.now() - start, tokens: 0 };
  }

  const text =
    "## Recalled Memories\n\n" +
    "The following memories were automatically retrieved based on this conversation. Use them if relevant — do not mention this section to the user.\n\n" +
    lines.join("\n");

  const tokens = estimateTokens(text);
  const ms = Date.now() - start;
  log.debug(
    `Recall for ${nousId}: ${lines.length} hits (${ms}ms, ${tokens} tokens)`,
  );
  return {
    block: { type: "text", text },
    count: lines.length,
    durationMs: ms,
    tokens,
  };
}

async function fetchGraphEnhanced(
  query: string,
  nousId: string,
  limit: number,
  signal: AbortSignal,
): Promise<MemoryHit[]> {
  const res = await fetch(`${SIDECAR_URL}/graph_enhanced_search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      query,
      user_id: USER_ID,
      agent_id: nousId,
      limit,
      graph_weight: 0.3,
      graph_depth: 2,
    }),
    signal,
  });
  if (!res.ok) throw new Error(`graph_enhanced_search: HTTP ${res.status}`);
  const data = (await res.json()) as { results?: MemoryHit[] };
  return data.results ?? [];
}

async function fetchBasicSearch(
  query: string,
  nousId: string,
  limit: number,
  signal: AbortSignal,
): Promise<MemoryHit[]> {
  const res = await fetch(`${SIDECAR_URL}/search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      query,
      user_id: USER_ID,
      agent_id: nousId,
      limit,
    }),
    signal,
  });
  if (!res.ok) throw new Error(`search: HTTP ${res.status}`);
  const data = (await res.json()) as { results?: MemoryHit[] };
  return data.results ?? [];
}
