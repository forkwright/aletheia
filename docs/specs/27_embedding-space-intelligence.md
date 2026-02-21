# Spec 27: Embedding-Space Intelligence — JEPA Principles for Agent Architecture

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-20
**Spec:** 27

---

## Problem

Aletheia processes every turn uniformly: full bootstrap assembly, memory search, fact extraction, signal classification — regardless of whether the incoming message meaningfully changes the conversation state. A "yes" after a complex technical exchange triggers the same pipeline cost as the original complex request. Word overlap at 10% triggers a topic_change signal even when the semantic content is continuous. Similarity pruning uses Jaccard word overlap when the system already has access to dense 1024-dimensional Voyage embeddings. Contradiction detection relies on negation word asymmetry when embedding geometry carries richer semantic relationships.

The core insight from JEPA (Joint Embedding Predictive Architecture) research is that operating in representation space — dense embeddings that capture semantic meaning — is fundamentally more efficient and informative than operating in token space. Meta's VL-JEPA achieves 50% fewer trainable parameters and 2.85x decoding efficiency by predicting in embedding space rather than token space. The JEPA family (I-JEPA, V-JEPA 2, VL-JEPA) demonstrates that abstraction-level operations outperform surface-level operations across vision, video, and language tasks.

Aletheia already embeds every memory via Voyage-3-large. But embedding access is confined to the sidecar at storage and search time. The runtime itself is embedding-blind: it reasons about text with regex patterns, word tokenization, and character-level heuristics when the semantic signal is available and unused.

### Concrete Waste

1. **Uniform turn cost.** A "thanks" message triggers: bootstrap assembly (~40ms), recall query to Qdrant (~200-500ms), interaction signal classification, working state extraction (Haiku call, ~200ms), and turn fact extraction (Haiku call, ~200ms). For messages that don't shift the conversation semantically, the recall and extraction are wasted compute.

2. **Lossy similarity metrics.** The MMR diversity selection in `recall.ts` and the similarity pruning in `similarity-pruning.ts` both use Jaccard word overlap. "The truck's steering is loose" vs "The Ram 2500 has play in the steering column" share few words but are semantically identical. Jaccard says 0.15 (different), cosine on Voyage embeddings would say 0.92 (same).

3. **Naive contradiction detection.** `_check_contradictions` in the sidecar uses negation word sets. "Pitman arm torque is 185 ft-lbs" vs "Pitman arm torque is 225 ft-lbs" has no negation asymmetry — both are affirmative statements. They share high cosine similarity but differ on a key value. The contradiction is invisible to the current heuristic.

4. **No cross-agent semantic routing.** `bestAgentForDomain()` uses string-matched domain labels from config. If a message about "leather tooling techniques" arrives, the competence model needs an exact domain match. Embedding the message and comparing to each agent's memory cluster centroid would reveal that Demiurge's memory space is closest — without config labels.

5. **No predictive context.** Every turn starts fresh: what tools will this need? What memories are relevant? Past turns with similar embeddings already answered these questions. The answers are in the session history but unexploited.

---

## Background: JEPA Principles

Six principles from Meta's JEPA research that transfer directly to agent architecture:

| Principle | JEPA Origin | Aletheia Application |
|-----------|------------|---------------------|
| **Selective decoding** | VL-JEPA skips redundant video frames via semantic shift detection (2.85x efficiency) | Skip recall/extraction for semantically unchanged turns |
| **Representation space** | Predict embeddings, not tokens — captures semantic structure that surface analysis misses | Replace Jaccard word overlap with cosine similarity across pipeline |
| **World model** | Use past states to predict future needs (Mode-2 deliberative reasoning) | Past turn patterns predict tools + memories needed for current turn |
| **Joint embedding** | Shared embedding space aligns different modalities/sources | Cross-agent memory discovery via embedding proximity |
| **Hierarchical planning** | H-JEPA: high-level goals decompose into subgoals, track progress as embedding movement | Goal vectors with drift detection and re-centering |
| **Collapse prevention** | VICReg regularization maintains embedding space health (variance + decorrelation) | Monitor embedding space for clustering collapse and model drift |

Sources: [VL-JEPA (arXiv:2512.10942)](https://arxiv.org/abs/2512.10942), [V-JEPA 2](https://ai.meta.com/vjepa/), [LeCun Position Paper](https://openreview.net/pdf?id=BZ5a1r-kVsf), [JEPA-Reasoner (arXiv:2512.19171)](https://www.alphaxiv.org/overview/2512.19171v1)

---

## Design

### Principles

1. **Embed once, query many.** Every incoming message gets embedded exactly once at pipeline entry. That vector is reused for shift detection, recall, signal classification, and any other embedding-space operation in the turn.

2. **Text decoding only at boundaries.** Internal pipeline decisions (should we search memory? which tools to load? is this a topic change?) use embedding geometry. Text-level analysis is reserved for external communication and human-readable outputs.

3. **Existing infrastructure, new wiring.** Voyage-3-large embedding via the sidecar. Qdrant for vector storage and search. No new services, no new models, no new dependencies.

4. **Graceful degradation.** If the sidecar is down, the pipeline falls back to current behavior (text heuristics). Embedding operations are never blocking or required.

5. **Measurable improvement.** Each phase has a concrete efficiency or accuracy metric. No speculative "intelligence" claims — measured improvements or revert.

### Architecture

Current pipeline flow:
```
Message → resolve → guard → context [bootstrap + recall + injections] → history → execute → finalize
```

Each stage operates on text. Recall queries the sidecar but doesn't retain the query embedding. Signal classification uses regex. Similarity pruning uses word sets.

Target flow:
```
Message → embed → resolve → guard → context [shift-gated recall + embedding-informed injections] → history → execute → finalize
```

The key addition is a `TurnEmbedding` that travels with the `TurnState` through all stages:

```typescript
interface TurnEmbedding {
  vector: number[];          // 1024d Voyage-3-large
  timestamp: number;
  cachedSimilarities?: {
    lastTurnEmbedding?: number;    // cosine to previous turn
    sessionCentroid?: number;       // cosine to running session average
  };
}
```

Added to `TurnState` as an optional field. Stages that currently use text heuristics gain an embedding-space alternative with fallback to current behavior.

---

## Phases

### Phase 1: Semantic Shift Detection (Selective Processing)

**JEPA principle:** Selective decoding — monitor embedding stream, only trigger full processing when semantic state changes meaningfully. VL-JEPA achieves 2.85x efficiency by detecting semantic shifts in video embedding streams and only decoding at shift boundaries.

**Scope:** Embed each incoming message and compare to the previous turn's embedding. Use the cosine distance to gate expensive operations (memory recall, turn fact extraction). Low-shift turns (acknowledgments, confirmations, followups on same topic) skip these stages.

**Changes:**

1. **NEW `src/nous/embedding.ts`** (~80 LOC) — Embedding utilities:
   - `embedMessage(text: string): Promise<TurnEmbedding | undefined>` — calls sidecar `/embed` endpoint
   - `cosineSimilarity(a: number[], b: number[]): number` — dot product on unit vectors
   - `embedBatch(texts: string[]): Promise<number[][]>` — batch embed for efficiency
   - Fallback: if sidecar unreachable, returns undefined (stages fall through to current behavior)

2. **NEW sidecar endpoint `POST /embed`** in `routes.py` (~30 LOC) — Accept text, return vector. Simpler than `/add` — just embed, no storage. Avoids runtime needing its own Voyage API key.

3. **MODIFY `src/nous/pipeline/stages/context.ts`** — Before recall, compute semantic shift:
   ```typescript
   const embedding = await embedMessage(msg.text);
   state.embedding = embedding;
   const lastEmbedding = services.store.getLastTurnEmbedding(sessionId);
   const shift = lastEmbedding
     ? 1.0 - cosineSimilarity(embedding.vector, lastEmbedding)
     : 1.0;
   state.trace.setSemanticShift(shift);

   const RECALL_THRESHOLD = 0.15;   // Below: reuse previous recall
   const EXTRACT_THRESHOLD = 0.10;  // Below: skip turn fact extraction
   ```

4. **MODIFY `src/nous/pipeline/stages/finalize.ts`** — Gate turn fact extraction on semantic shift. If shift < EXTRACT_THRESHOLD, skip extraction (the response to "yes" rarely contains durable facts).

5. **MODIFY `src/mneme/store.ts`** — Add `updateLastTurnEmbedding(sessionId, vector)` and `getLastTurnEmbedding(sessionId)`. Store as BLOB in sessions table (1024 floats = 4KB). New migration adds `last_turn_embedding` column.

**Acceptance Criteria:**
- [ ] Every incoming message gets a 1024d embedding vector (when sidecar available)
- [ ] Semantic shift computed as cosine distance to previous turn
- [ ] Recall skipped for turns with shift < 0.15 (previous recall reused)
- [ ] Turn fact extraction skipped for turns with shift < 0.10
- [ ] Trace includes semantic shift value for observability
- [ ] Fallback to current behavior when sidecar unavailable

**Testing:**
- "yes" after complex question → shift < 0.10 → extraction skipped
- Topic change → shift > 0.4 → full pipeline runs
- Followup on same topic → shift 0.10-0.30 → recall skipped, extraction runs
- 10-turn conversation with 3 topic changes → recall runs 4 times (3 shifts + first turn)
- Benchmark: Haiku calls per 10-turn conversation reduced by ~40%

**LOC:** ~250 (80 embedding.ts + 30 context.ts + 15 finalize.ts + 20 store.ts + 15 migration + 30 sidecar + 60 tests)

---

### Phase 2: Embedding-Space Memory Operations

**JEPA principle:** Predict in representation space, not token space. Move similarity computations from word-level heuristics to embedding geometry. Dense embeddings capture semantic relationships that surface-level text analysis misses.

**Scope:** Replace three text-based heuristics with embedding-based equivalents using the same Voyage vectors already stored in Qdrant.

**Changes:**

1. **MODIFY `src/nous/recall.ts`** — Replace Jaccard MMR with cosine MMR:
   ```typescript
   export function mmrSelectCosine(
     candidates: MemoryHitWithVector[],
     limit: number,
     lambda = 0.7,
   ): MemoryHitWithVector[] {
     // Same MMR algorithm, similarity = cosine(embedding_a, embedding_b)
     // instead of jaccard(words_a, words_b)
   }
   ```
   Requires sidecar `/search` to return vectors alongside results. Qdrant supports `with_vectors=true` — one-line sidecar change + type extension in runtime.

2. **MODIFY `src/distillation/similarity-pruning.ts`** — Replace Jaccard window similarity with embedding similarity:
   ```typescript
   export async function pruneBySimilarityEmbedding(
     messages: SimpleMessage[],
     embedFn: (texts: string[]) => Promise<number[][]>,
     opts?: { windowSize?: number; overlapThreshold?: number; minMessages?: number },
   ): Promise<PruningResult>
   ```
   Adds ~50ms latency per batch embed call, acceptable during background distillation. Fallback: if embed fails, fall through to existing Jaccard pruning.

3. **MODIFY sidecar `routes.py`** — Improve contradiction detection:
   ```python
   async def _check_contradictions_v2(client, vector, text, user_id):
       # 1. Find high-similarity memories (cosine > 0.80)
       # 2. For candidates 0.80-0.90 (same topic, different content):
       #    Extract key values from both texts
       #    If entities match but values differ → contradiction
       # 3. Haiku call only for ambiguous cases (cosine 0.85-0.90)
   ```
   Catches "torque is 185 ft-lbs" vs "torque is 225 ft-lbs" without requiring negation words.

**Acceptance Criteria:**
- [ ] MMR diversity selection uses cosine similarity (vectors from Qdrant results)
- [ ] Recall returns vectors alongside memory text (sidecar change)
- [ ] Distillation pruning uses embedding similarity (with Jaccard fallback)
- [ ] Contradiction detection catches value-different same-topic conflicts
- [ ] No regression in recall diversity (A/B: Jaccard MMR vs cosine MMR)

**Testing:**
- Two paraphrased memories → cosine MMR detects similarity, Jaccard misses it
- "Torque is 185" vs "torque is 225" → contradiction detected without negation words
- Pruning identifies semantically redundant messages across paraphrases
- Recall diversity score (unique topics in top-8) improves by 15%+

**LOC:** ~330 (60 recall.ts + 80 similarity-pruning.ts + 100 routes.py + 10 sidecar search + 80 tests)

---

### Phase 3: Predictive Context Assembly (World Model Lite)

**JEPA principle:** World model for planning via simulation. Use memory of past interactions to predict outcomes before acting. LeCun's Mode-2 deliberative reasoning: simulate future states, evaluate costs, optimize action sequences.

**Scope:** When a new message arrives, find historically similar messages and check what those turns actually used (which tools, which memories, how many tokens of thinking). Use that to pre-configure the current turn.

**Changes:**

1. **MODIFY `src/nous/pipeline/stages/context.ts`** — Predictive tool pre-loading:
   ```typescript
   const similarTurns = services.store.findSimilarTurns(embedding.vector, nousId, { limit: 5 });
   const predictedTools = aggregateToolUsage(similarTurns);
   for (const toolName of predictedTools.slice(0, 3)) {
     services.tools.enableTool(toolName, sessionId, seq);
   }
   ```

2. **MODIFY `src/mneme/store.ts`** — New `turn_embeddings` table (migration):
   ```sql
   CREATE TABLE turn_embeddings (
     id INTEGER PRIMARY KEY,
     session_id TEXT NOT NULL,
     nous_id TEXT NOT NULL,
     turn_seq INTEGER NOT NULL,
     embedding BLOB NOT NULL,
     tools_used TEXT,
     memory_queries TEXT,
     thinking_tokens INTEGER,
     created_at TEXT DEFAULT (datetime('now'))
   );
   CREATE INDEX idx_turn_embeddings_nous ON turn_embeddings(nous_id);
   ```
   Vector search via brute-force cosine in-process (table is small, ~1000 rows per agent after retention).

3. **MODIFY `src/hermeneus/complexity.ts`** — Embedding-informed complexity scoring:
   ```typescript
   export function scoreComplexityWithHistory(
     opts: ComplexityOpts,
     history?: { avgThinkingTokens: number; avgToolCalls: number },
   ): ComplexityResult
   ```
   If similar past turns had high thinking token usage → bias toward "complex" tier.

4. **MODIFY `src/nous/pipeline/stages/finalize.ts`** — Store turn embedding with tool/thinking metadata after each turn.

**Acceptance Criteria:**
- [ ] Similar past turns found by embedding (brute-force cosine on local SQLite)
- [ ] Predicted tools pre-enabled before LLM call
- [ ] Complexity scoring informed by historical turn patterns
- [ ] Turn embeddings stored with tool/thinking metadata
- [ ] Retention: keep last 1000 embeddings per agent, prune oldest

**Testing:**
- Message similar to a past tool-heavy turn → tools pre-loaded
- Message similar to a past simple turn → routine complexity tier
- After 20 turns, predictive tool loading matches actual usage 60%+
- `enable_tool` calls from agent reduced by 30%+

**LOC:** ~260 (50 context.ts + 80 store.ts + 30 complexity.ts + 20 finalize.ts + 20 migration + 60 tests)

---

### Phase 4: Cross-Agent Shared Embedding Space

**JEPA principle:** Joint embedding for cross-modal/cross-agent understanding. VL-JEPA aligns vision and language in a shared 1,536d space. Applied here: formalize the existing single-collection Qdrant as an intentional shared embedding space where agent boundaries are data-driven, not label-driven.

**Scope:** Add cross-agent memory discovery via embedding proximity. Use agent memory cluster centroids for semantic routing.

**Changes:**

1. **NEW sidecar endpoint `POST /search_cross_agent`** in `routes.py`:
   ```python
   # Search all agents' memories, group results by agent_id
   # Reveals which agents know about a topic, ranked by relevance
   ```

2. **NEW sidecar endpoint `GET /agent_centroids`** in `routes.py`:
   ```python
   # Compute/cache centroid (mean embedding) per agent's memory space
   # Semantic fingerprint for each agent's knowledge domain
   # Cached 1 hour, recomputed on POST
   ```

3. **MODIFY `src/nous/competence.ts`** — Add `bestAgentByEmbedding()`:
   ```typescript
   async bestAgentByEmbedding(
     queryVector: number[],
     exclude?: string[],
   ): Promise<{ nousId: string; score: number } | null>
   ```
   Queries sidecar `/agent_centroids`, finds agent whose memory centroid is closest to query.

4. **MODIFY `src/nous/pipeline/stages/context.ts`** — Use embedding-based routing for delegation suggestions.

**Acceptance Criteria:**
- [ ] Cross-agent search returns memories from all agents grouped by agent_id
- [ ] Agent centroids computed and cached (per-agent mean embedding)
- [ ] Embedding-based routing suggests delegation based on memory proximity
- [ ] No regression in single-agent recall performance

**Testing:**
- Query about "leather working" → Demiurge centroid is closest
- Query about "health scheduling" → Chiron centroid is closest
- `/search_cross_agent` for "vehicle" → Akron's memories ranked first
- Centroid cache invalidation after 10+ new memories

**LOC:** ~250 (80 search_cross_agent + 60 agent_centroids + 40 competence.ts + 20 context.ts + 50 tests)

---

### Phase 5: Hierarchical Goal Tracking (H-JEPA Pattern)

**JEPA principle:** Hierarchical multi-timescale planning. H-JEPA stacks predictors at different abstraction levels — higher levels set subgoals for lower levels. Track progress as movement through embedding space toward a goal state.

**Scope:** Track conversation goals as embedding vectors. Detect goal drift (turns moving away from intent) and goal completion (turns converging to goal region).

**Changes:**

1. **NEW `src/nous/goal-tracker.ts`** (~120 LOC):
   ```typescript
   interface GoalState {
     id: string;
     goalText: string;
     goalVector: number[];
     startTurnSeq: number;
     progressHistory: Array<{ turnSeq: number; distance: number; delta: number }>;
     status: "active" | "completed" | "drifted" | "abandoned";
   }

   export class GoalTracker {
     // Detect goals from complex first-messages or topic changes
     // Track: each turn's embedding distance to goal vector
     // Drift: 3+ consecutive turns with increasing distance
     // Completion: distance drops below 0.2
   }
   ```

2. **MODIFY `src/nous/pipeline/stages/context.ts`** — Inject drift warning:
   ```
   [System: Recent responses are drifting from the user's original intent:
   "{goal text}". Distance: 0.45 (was 0.25 three turns ago).
   Consider re-centering on the original request.]
   ```

3. **MODIFY `src/nous/pipeline/stages/finalize.ts`** — Update goal tracking with turn's embedding distance.

4. **MODIFY `src/mneme/store.ts`** — New `goals` table:
   ```sql
   CREATE TABLE goals (
     id TEXT PRIMARY KEY,
     session_id TEXT NOT NULL,
     nous_id TEXT NOT NULL,
     goal_text TEXT NOT NULL,
     goal_embedding BLOB NOT NULL,
     status TEXT DEFAULT 'active',
     start_turn INTEGER NOT NULL,
     progress TEXT,
     created_at TEXT DEFAULT (datetime('now')),
     completed_at TEXT
   );
   ```

**Acceptance Criteria:**
- [ ] Complex/first-message turns create a goal vector
- [ ] Turn progress tracked as cosine distance to goal
- [ ] Drift detected after 3+ consecutive distance increases
- [ ] Drift warning injected into system prompt
- [ ] Goal completion detected when distance < 0.2 + approval signal
- [ ] Goals visible in `status_report` tool output

**Testing:**
- 5 turns progressively closer to goal → no drift warning
- 3 turns diverging → drift warning injected
- Start debugging task, drift into unrelated topic → warning fires
- Goal completed → status set, no further tracking

**LOC:** ~255 (120 goal-tracker.ts + 25 context.ts + 15 finalize.ts + 30 store.ts + 15 migration + 50 tests)

---

### Phase 6: Embedding Health and Collapse Prevention

**JEPA principle:** Collapse prevention via VICReg (Variance-Invariance-Covariance) regularization. In JEPA training, representations can degenerate to a constant if not actively monitored. Applied here: monitor Aletheia's embedding space for clustering collapse, dimension atrophy, and model drift.

**Scope:** Add monitoring endpoints and a periodic health check. Detect degradation and trigger corrective actions.

**Changes:**

1. **NEW sidecar endpoint `GET /embedding_health`** in `routes.py` (~150 LOC):
   ```python
   # Sample 500 vectors from Qdrant
   # Compute:
   #   variance: mean per-dimension variance (healthy: > 0.01)
   #   cluster_count: estimated clusters via k-means (healthy: > 5)
   #   max_cluster_density: % in densest cluster (healthy: < 40%)
   #   dimension_utilization: % dimensions with significant variance (healthy: > 80%)
   #   drift_score: similarity between old (>30d) and new embeddings on same entities
   #     (healthy: > 0.85, drop indicates embedder model change)
   ```

2. **MODIFY `src/daemon/cron.ts`** — Weekly embedding health check:
   ```typescript
   { id: "embedding-health", command: "embedding:health", schedule: "0 5 * * 0" }
   ```

3. **Wire existing `reindex.py`** to automated trigger — when drift_score drops below 0.7, fire `/reindex` to re-embed all memories with current embedder.

4. **Prosoche integration** — Surface embedding health warnings in PROSOCHE.md when metrics degrade.

**Acceptance Criteria:**
- [ ] `/embedding_health` returns variance, cluster count, density, dimension utilization, drift score
- [ ] Weekly cron check logs results
- [ ] Unhealthy metrics trigger PROSOCHE.md warning
- [ ] Drift detection fires when embedder model changes
- [ ] Reindex endpoint re-embeds all memories (background, non-blocking)

**Testing:**
- Healthy embedding space → all metrics pass
- Artificially collapsed space (near-identical vectors) → variance warning
- Artificially drifted space → drift warning
- Reindex processes 100 test memories without error

**LOC:** ~280 (150 embedding_health + 30 cron + 20 prosoche + 40 reindex wiring + 40 tests)

---

## Implementation Order

| Phase | Principle | Effort | Impact | Dependencies |
|-------|-----------|--------|--------|-------------|
| **1: Semantic Shift Detection** | Selective processing | ~250 LOC | High — ~40% fewer Haiku calls | None |
| **2: Embedding-Space Memory Ops** | Representation space | ~330 LOC | High — better diversity, catch paraphrases | Phase 1 |
| **6: Embedding Health** | Collapse prevention | ~280 LOC | Low (maintenance) — prevents silent degradation | None |
| **3: Predictive Context Assembly** | World model | ~260 LOC | Medium — fewer enable_tool calls, better routing | Phase 1 |
| **4: Cross-Agent Embedding Space** | Joint embedding | ~250 LOC | Medium — data-driven agent routing | Phase 1 |
| **5: Hierarchical Goal Tracking** | H-JEPA planning | ~255 LOC | Medium — drift detection, conversation steering | Phase 1 |
| **Total** | | **~1,625** | | |

Phases 1 and 6 have no dependencies — start immediately. Phase 1 is the foundation. Phases 2-5 are independent of each other once Phase 1 ships.

---

## Open Questions

1. **Embedding latency budget.** The sidecar embed call adds ~50-100ms per turn. Acceptable for shift detection? Alternative: cache the embedder in the runtime process (requires Voyage API key in runtime env).

2. **Turn embedding storage size.** 1024 floats × 4 bytes = 4KB per turn. At 200 turns/day → 800KB/day, ~24MB/month. Acceptable for SQLite? Or move to Qdrant?

3. **Shift threshold calibration.** 0.15 and 0.10 are initial guesses. Auto-calibrate by tracking shift distributions over a week and setting thresholds at the 25th and 10th percentiles?

4. **Goal detection accuracy.** Not every complex message is a trackable goal. Filter via embedding similarity to known non-goal patterns? Or lightweight Haiku classification?

5. **Centroid cache invalidation.** How often do agent centroids shift enough to matter? Recompute hourly? On every N adds? On explicit trigger?

---

## References

| Spec | Relationship |
|------|-------------|
| **16** (Efficiency) | Phase 1 extends efficiency gains — shift detection skips more work than dynamic thinking alone |
| **17** (Gap Analysis) | F-6 MMR diversity addressed by Phase 2's cosine MMR |
| **19** (Sleep-Time Compute) | Phase 6 health check runs alongside nightly reflection |
| **23** (Memory Pipeline) | Phase 2 improves the recall and contradiction systems that spec 23 built |
| **26** (Recursive Self-Improvement) | Phase 3 predictive context is self-improvement — system learns from its own turn patterns |

### External Sources

- [VL-JEPA: Joint Embedding Predictive Architecture for Vision-language (arXiv:2512.10942)](https://arxiv.org/abs/2512.10942)
- [V-JEPA 2: Meta AI](https://ai.meta.com/vjepa/)
- [A Path Towards Autonomous Machine Intelligence (LeCun, 2022)](https://openreview.net/pdf?id=BZ5a1r-kVsf)
- [JEPA-Reasoner (arXiv:2512.19171)](https://www.alphaxiv.org/overview/2512.19171v1)
- [VICReg: Variance-Invariance-Covariance Regularization (arXiv:2105.04906)](https://arxiv.org/abs/2105.04906)
- [V-JEPA 2 GitHub](https://github.com/facebookresearch/vjepa2)
- [EB-JEPA GitHub](https://github.com/facebookresearch/eb_jepa)
