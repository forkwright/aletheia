// Pre-turn memory recall — surfaces relevant memories before LLM reasoning
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";

const log = createLogger("recall");

// Lazy reads — env vars may be set by taxis config after module import
const getSidecarUrl = () => process.env["ALETHEIA_MEMORY_URL"] ?? "http://127.0.0.1:8230";
const getUserId = () => process.env["ALETHEIA_MEMORY_USER"] ?? "default";

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

export interface RecallOpts {
  limit?: number;
  maxTokens?: number;
  timeoutMs?: number;
  minScore?: number;
  /** Score threshold at which vector results are considered "sufficient" — skips graph fallback. */
  sufficiencyThreshold?: number;
  /** Minimum number of hits above sufficiencyThreshold to skip graph fallback. */
  sufficiencyMinHits?: number;
  domains?: string[];
  threadSummary?: string;
}

export async function recallMemories(
  messageText: string,
  nousId: string,
  opts?: RecallOpts,
): Promise<RecallResult> {
  const limit = opts?.limit ?? 8;
  const maxTokens = opts?.maxTokens ?? 1500;
  const timeoutMs = opts?.timeoutMs ?? 5000;
  const minScore = opts?.minScore ?? 0.75;
  // Sufficiency gates: if N hits score above this threshold, vector search alone
  // is sufficient — skip the expensive graph-enhanced fallback entirely.
  const sufficiencyThreshold = opts?.sufficiencyThreshold ?? 0.85;
  const sufficiencyMinHits = opts?.sufficiencyMinHits ?? 3;
  const start = Date.now();

  // Thread-aware query: combine current message (70%) with thread summary (30%)
  // Thread summary provides broader context so recall isn't limited to the last message
  const msgQuery = messageText.slice(0, 500);
  const query = opts?.threadSummary
    ? `${msgQuery}\n\nThread context: ${opts.threadSummary.slice(0, 300)}`
    : msgQuery;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  let hits: MemoryHit[] = [];

  try {
    // Tier 1: Vector-only search (fast, ~200-500ms)
    hits = await fetchBasicSearch(query, nousId, limit, controller.signal, opts?.domains);

    // Sufficiency gate: count hits that score above the high-confidence threshold.
    // If enough strong hits exist, vector search alone is sufficient — no need for
    // the expensive graph traversal (1-2s). This is the "tiered retrieval" pattern
    // from memU: only escalate to deeper retrieval when shallow results are weak.
    const strongHits = hits.filter(
      (h) => h.score !== null && h.score !== undefined && h.score >= sufficiencyThreshold,
    );
    const isSufficient = strongHits.length >= sufficiencyMinHits;

    if (isSufficient) {
      log.debug(
        `Recall sufficiency gate passed for ${nousId}: ${strongHits.length} hits above ${sufficiencyThreshold} — skipping graph fallback`,
      );
    } else {
      // Check if we have ANY usable hits (above minScore but below sufficiency)
      const hasUsableHits = hits.some(
        (h) => h.score !== null && h.score !== undefined && h.score >= minScore,
      );

      if (!hasUsableHits) {
        // Tier 2: Graph-enhanced search — only when vector search found nothing useful
        const elapsed = Date.now() - start;
        const remaining = timeoutMs - elapsed;
        if (remaining > 1000) {
          log.debug(
            `Vector search returned no hits above ${minScore} for ${nousId}, trying graph-enhanced (${remaining}ms remaining)`,
          );
          const graphHits = await fetchGraphEnhanced(
            query,
            nousId,
            limit,
            controller.signal,
          );
          hits = graphHits;
        }
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

  // Exact-text dedup first, then MMR diversity selection
  const seen = new Set<string>();
  const exactDeduped: MemoryHit[] = [];
  for (const h of filtered) {
    if (!seen.has(h.memory)) {
      seen.add(h.memory);
      exactDeduped.push(h);
    }
  }

  // MMR-style diversity selection: penalize candidates similar to already-selected items
  // Uses token-level Jaccard overlap (no vector fetch needed, zero latency cost)
  const deduped = mmrSelect(exactDeduped, limit);

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

/**
 * MMR-style diversity selection using token-level Jaccard overlap.
 * Greedily selects the highest-scoring candidate that isn't too similar
 * to already-selected items. This prevents "5 memories that all say
 * the same thing" without requiring vector access.
 *
 * @param lambda - Balance between relevance (1.0) and diversity (0.0). Default 0.7.
 */
export function mmrSelect(
  candidates: MemoryHit[],
  limit: number,
  lambda = 0.7,
): MemoryHit[] {
  if (candidates.length <= 1) return candidates.slice(0, limit);

  const selected: MemoryHit[] = [];
  const remaining = [...candidates];
  const tokenCache = new Map<string, Set<string>>();

  function getTokens(text: string): Set<string> {
    let cached = tokenCache.get(text);
    if (!cached) {
      cached = new Set(text.toLowerCase().split(/\s+/).filter((t) => t.length > 2));
      tokenCache.set(text, cached);
    }
    return cached;
  }

  function jaccardSimilarity(a: string, b: string): number {
    const tokensA = getTokens(a);
    const tokensB = getTokens(b);
    if (tokensA.size === 0 && tokensB.size === 0) return 1;
    let intersection = 0;
    for (const t of tokensA) {
      if (tokensB.has(t)) intersection++;
    }
    const union = tokensA.size + tokensB.size - intersection;
    return union === 0 ? 0 : intersection / union;
  }

  // First item: always pick the highest-scoring
  selected.push(remaining.shift()!);

  while (selected.length < limit && remaining.length > 0) {
    let bestIdx = 0;
    let bestMmrScore = -Infinity;

    for (let i = 0; i < remaining.length; i++) {
      const candidate = remaining[i]!;
      const relevance = candidate.score ?? 0;

      // Max similarity to any already-selected item
      let maxSim = 0;
      for (const sel of selected) {
        const sim = jaccardSimilarity(candidate.memory, sel.memory);
        if (sim > maxSim) maxSim = sim;
      }

      // MMR score: balance relevance and diversity
      const mmrScore = lambda * relevance - (1 - lambda) * maxSim;
      if (mmrScore > bestMmrScore) {
        bestMmrScore = mmrScore;
        bestIdx = i;
      }
    }

    selected.push(remaining.splice(bestIdx, 1)[0]!);
  }

  return selected;
}

async function fetchGraphEnhanced(
  query: string,
  nousId: string,
  limit: number,
  signal: AbortSignal,
): Promise<MemoryHit[]> {
  const res = await fetch(`${getSidecarUrl()}/graph_enhanced_search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      query,
      user_id: getUserId(),
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
  domains?: string[],
): Promise<MemoryHit[]> {
  const res = await fetch(`${getSidecarUrl()}/search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      query,
      user_id: getUserId(),
      agent_id: nousId,
      limit,
      ...(domains && domains.length > 0 ? { domains } : {}),
    }),
    signal,
  });
  if (!res.ok) throw new Error(`search: HTTP ${res.status}`);
  const data = (await res.json()) as { results?: MemoryHit[] };
  return data.results ?? [];
}
