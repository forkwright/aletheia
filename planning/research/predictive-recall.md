# Predictive Recall System

**Date:** 2026-03-17
**Author:** Research agent
**Status:** Proposal
**Closes:** #1467

## Question

Current recall is purely reactive: each turn embeds the user message, runs tiered search (HNSW + BM25 + graph), scores via the 6-factor engine, and injects results into the system prompt. No look-ahead occurs. Can we anticipate what the agent will need before the user finishes typing, reducing perceived latency and improving recall quality?

---

## Findings

### 1. Current recall pipeline

The recall pipeline executes synchronously within the turn lifecycle:

```
User message arrives
  → RecallStage::run()                    [nous/src/recall.rs:305]
    → embed(query)                        [hermeneus embedding provider]
    → search_vectors(k=45, ef=50)         [mneme HNSW, <10ms]
    → build_candidates() + rank()         [6-factor scoring]
    → (optional) run_iterative()          [terminology discovery + cycle 2]
  → format + inject into system prompt
  → Execute stage (LLM call)
```

**Timing breakdown (typical):**

| Stage | Latency |
|-------|---------|
| Embedding (MiniLM, 384-dim) | 5-15ms local, 50-100ms remote |
| HNSW kNN (k=15, ef=50) | <10ms |
| BM25 search | <5ms |
| Hybrid + RRF merge (Tier 1) | <50ms |
| LLM query rewrite (Tier 2) | 50-200ms |
| Graph expansion (Tier 3) | 20-50ms |
| 6-factor scoring + ranking | <5ms |
| **Total (Tier 1 fast path)** | **20-60ms** |
| **Total (Tier 3 worst case)** | **200-400ms** |

The fast path is already sub-100ms. Predictive recall targets the Tier 2/3 cases and the qualitative gap where reactive retrieval misses context the agent needs mid-generation.

**Signals used today:**

1. **Vector similarity** (0.35 weight)  -  cosine distance in MiniLM embedding space
2. **FSRS temporal decay** (0.20)  -  power-law forgetting curve tuned by fact type and epistemic tier
3. **Nous relevance** (0.15)  -  own vs shared vs other agent memories
4. **Epistemic tier** (0.15)  -  verified > inferred > assumed
5. **Graph proximity** (0.10)  -  BFS hop count from query entities, community-boosted
6. **Access frequency** (0.05)  -  log-normalized recall count with supersession chain bonus

**Key observation:** Only vector similarity is computed from the actual query. The other five factors are either static per-fact properties or config defaults. This means the scoring function is mostly query-independent once candidates are retrieved  -  a property that makes pre-fetching viable.

### 2. predictive recall strategies

#### Strategy a: graph neighborhood pre-Loading

**Mechanism:** When the current turn mentions entities E1, E2, ... En, pre-fetch the 1-hop and 2-hop neighborhoods of those entities from the knowledge graph before the next turn arrives.

**Implementation sketch:**
- After extraction completes (background task), identify entities mentioned in the turn.
- Issue `relationships{src: entity, dst: neighbor}` queries for each entity.
- Load associated facts into a warm cache (bounded LRU).
- On next recall, check warm cache before hitting HNSW.

**Strengths:**
- Exploits the existing graph structure (entities, relationships, community clusters).
- Low marginal cost  -  Cozo Datalog queries are sub-millisecond for 1-hop.
- High hit rate for conversations that stay within a topic cluster.

**Weaknesses:**
- Useless if conversation shifts to an unrelated topic.
- Requires extraction to have run (extraction is async, may not finish before next turn).
- 2-hop neighborhoods can be large for hub entities (PageRank > 0.1).

**Estimated hit rate:** 40-60% for multi-turn conversations on a single topic, <10% for topic-switching turns.

#### Strategy b: conversation trajectory prediction

**Mechanism:** Use the last N turns to predict likely next-turn topics via a lightweight model (n-gram, TF-IDF, or small LM).

**Implementation sketch:**
- Maintain a sliding window of the last 5-10 turn embeddings.
- Compute a "trajectory vector" by weighted average of recent embeddings (exponential decay, most recent = highest weight).
- Pre-embed the trajectory vector and run HNSW search during idle time.
- Cache results as "speculative candidates."

**Strengths:**
- Captures topic drift naturally (trajectory vector moves with conversation).
- No dependency on extraction completion.
- Works even when graph is sparse.

**Weaknesses:**
- Trajectory vector is a coarse signal  -  it averages over topics rather than predicting the next one.
- Requires maintaining per-session embedding history (memory cost: ~1.5KB per turn at 384 dims).
- Speculative search adds background CPU/IO load.

**Estimated hit rate:** 30-50% depending on conversation coherence.

#### Strategy c: idle-Time pre-Fetching

**Mechanism:** While the user is composing their next message (detected via typing indicators or the gap between turns), run speculative recall using the current conversation context.

**Implementation sketch:**
- After the assistant response is delivered, immediately compute a "context summary" embedding from the last assistant message.
- Run Tier 1 search in the background.
- Store results in a per-session speculative cache.
- On next user message, merge speculative results with fresh recall results via RRF.

**Strengths:**
- Uses dead time (user thinking/typing) productively.
- No prediction model needed  -  the last assistant message is a reasonable proxy for what comes next.
- Minimal architecture change  -  same search pipeline, just triggered earlier.

**Weaknesses:**
- Only useful when there is a significant gap between turns (>500ms).
- The assistant's last message may not predict the user's next question.
- Cache invalidation needed if the speculative results become stale.

**Estimated hit rate:** 25-40%. Higher for follow-up questions, lower for topic switches.

#### Strategy d: user pattern learning

**Mechanism:** Track recurring topic sequences per user/session over time. If user frequently discusses A then B, pre-fetch B-related facts when A appears.

**Implementation sketch:**
- After extraction, log topic transitions as (previous_topics → current_topics) pairs.
- Build a per-nous transition frequency table (stored in knowledge store).
- When recall runs, look up the current topic in the transition table.
- Pre-fetch facts associated with the most likely successor topics.

**Strengths:**
- Learns user-specific patterns (e.g., always reviews auth after discussing sessions).
- Improves over time as more transitions are observed.
- High precision for habitual workflows.

**Weaknesses:**
- Requires significant history before patterns emerge (cold start problem).
- Topic classification itself is noisy (extraction must produce consistent entity/topic labels).
- Transition table grows with vocabulary; needs pruning strategy.
- Privacy consideration  -  stores behavioral patterns.

**Estimated hit rate:** 10-20% initially, potentially 40-60% after months of use.

#### Strategy e: heuristic - last n related conversations

**Mechanism:** When a session starts or after N turns, find the top-K most similar prior sessions and pre-load their extracted facts.

**Implementation sketch:**
- Embed the current session summary (or first N turn embeddings averaged).
- Search session-level embeddings (not fact-level) for similar past sessions.
- Bulk-load facts from those sessions into a warm cache.

**Strengths:**
- Low implementation cost  -  session embeddings likely already exist or are cheap to add.
- Good for recurring tasks ("last time I worked on X, I needed Y").
- Low false-positive rate  -  session-level similarity is a strong signal.

**Weaknesses:**
- Coarse granularity  -  loads entire sessions, not specific facts.
- Session embeddings may not exist yet (requires new indexing).
- Stale if the related session's facts have been superseded.

**Estimated hit rate:** 20-35% for users with recurring workflows.

### 3. comparison matrix

| Strategy | Hit Rate | Latency Saved | Compute Cost | Complexity | Cold Start |
|----------|----------|---------------|--------------|------------|------------|
| A: Graph neighborhood | 40-60% | 20-50ms (Tier 3) | Low | Low | No (graph exists) |
| B: Trajectory vector | 30-50% | 20-60ms | Medium | Medium | No (embeddings exist) |
| C: Idle pre-fetch | 25-40% | 50-200ms | Medium | Low | No |
| D: User patterns | 10-60% | 20-60ms | Low (after training) | High | Yes (months) |
| E: Session similarity | 20-35% | 20-50ms | Medium | Low | Mild (needs sessions) |

### 4. risks and mitigations

#### Risk 1: wasted compute on wrong predictions

Speculative recall consumes embedding compute, HNSW search time, and memory for results that may never be used.

**Mitigation:**
- Budget: cap speculative work at 50ms wall-clock per turn gap.
- Limit speculative cache to 20 facts (matching 4x the default `max_results=5`).
- Track hit rate per strategy. If a strategy's 30-day rolling hit rate drops below 15%, disable it automatically.
- Use the existing `access_count` mechanism: speculative results that are never injected get zero access bumps, naturally decaying their priority.

#### Risk 2: stale predictions after topic shift

If the user switches topics, speculative cache is useless and may even pollute results if merged naively.

**Mitigation:**
- Invalidate speculative cache when the cosine similarity between the new user message embedding and the speculative query embedding drops below 0.5.
- Never let speculative results override fresh recall  -  use RRF merge with speculative results getting a rank penalty (offset by +10 positions).
- TTL on speculative cache: expire after 2 turns unused.

#### Risk 3: memory pressure from pre-Fetched results

Large graph neighborhoods or aggressive pre-fetching could consume significant memory.

**Mitigation:**
- Bound the speculative cache at 20 facts × ~1KB = ~20KB per session. Negligible.
- For graph neighborhoods: limit to 1-hop only for entities with PageRank < 0.05; allow 2-hop only for high-importance entities.
- Evict speculative cache on session end.

#### Risk 4: increased complexity and debugging difficulty

Speculative recall adds a parallel code path that interacts with the existing 6-factor scoring system.

**Mitigation:**
- Keep speculative recall fully behind a feature flag (`recall.predictive.enabled`, default false).
- Emit structured tracing spans for all speculative operations: `recall.speculative.trigger`, `recall.speculative.hit`, `recall.speculative.miss`, `recall.speculative.invalidated`.
- Speculative results carry a `source: Speculative` tag in `ScoredResult` so they can be filtered in debugging.

#### Risk 5: accuracy regression

Pre-fetched results may lower average recall quality if low-relevance speculative results displace high-relevance fresh results.

**Mitigation:**
- Speculative results are candidates only  -  they still pass through the full 6-factor scoring pipeline.
- Apply a conservative score penalty (multiply speculative scores by 0.8) to prefer fresh results when scores are close.
- A/B test: run both reactive and predictive recall, compare quality metrics (mean reciprocal rank of facts the LLM actually references).

### 5. architecture sketch for MVP

```
                    Turn N completes
                         │
            ┌────────────┼────────────┐
            ▼            ▼            ▼
      Extraction    Speculative    User is
      (existing)    Pre-fetch      typing...
            │            │
            │     ┌──────┴──────┐
            │     │  Strategy A  │  Graph neighborhood
            │     │  1-hop fetch │  of extracted entities
            │     └──────┬──────┘
            │            │
            │     SpeculativeCache
            │     (bounded LRU, 20 facts)
            │            │
            ▼            ▼
                 Turn N+1 arrives
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
        Fresh Recall          Check Cache
        (existing pipeline)   (cosine gate ≥ 0.5)
              │                     │
              └──────────┬──────────┘
                         ▼
                    RRF Merge
                 (speculative rank +10 penalty)
                         │
                         ▼
                  6-Factor Scoring
                  (speculative × 0.8)
                         │
                         ▼
                  Inject into Prompt
```

**Components to add:**

1. **SpeculativeCache**  -  bounded LRU in `NousActor`, keyed by session ID.
   - `insert(session_id, query_embedding, Vec<ScoredResult>)`
   - `get_if_relevant(session_id, new_query_embedding, threshold=0.5) -> Option<Vec<ScoredResult>>`
   - `invalidate(session_id)`

2. **Speculative trigger** in `actor/background.rs`  -  after extraction spawns, also spawn `maybe_prefetch_neighborhood()`.
   - Gated on `config.recall.predictive.enabled`.
   - Queries 1-hop graph neighbors of entities from extraction results.
   - Loads associated facts and stores in `SpeculativeCache`.

3. **Merge logic** in `RecallStage::run()`  -  check cache before/after fresh recall, merge via RRF with rank penalty.

4. **Observability**  -  new tracing spans and metrics:
   - `recall.speculative.prefetch_count`
   - `recall.speculative.cache_hit` / `cache_miss`
   - `recall.speculative.invalidated`
   - `recall.speculative.injected_count`

### 6. effort estimate

| Work Item | Effort | Dependencies |
|-----------|--------|--------------|
| `SpeculativeCache` struct + tests | 1-2 days | None |
| Speculative trigger in `background.rs` | 1 day | Extraction pipeline (exists) |
| Graph neighborhood query in mneme | 0.5 days | KnowledgeStore (exists) |
| Merge logic in `RecallStage` | 1-2 days | SpeculativeCache |
| Feature flag in taxis config | 0.5 days | Config system (exists) |
| Tracing + metrics | 0.5 days | Tracing infrastructure (exists) |
| Integration tests | 1-2 days | All above |
| A/B evaluation framework | 2-3 days | Metrics pipeline |
| **Total MVP (Strategy A only)** | **7-11 days** | |

Adding Strategy B (trajectory vector) on top of the MVP: +3-5 days.
Adding Strategy C (idle pre-fetch) on top: +2-3 days.
Strategies D and E are recommended for a later phase after the MVP proves the cache/merge architecture.

---

## Recommendations

### Minimum viable approach

**Start with Strategy A (Graph Neighborhood Pre-Loading) alone.** Rationale:

1. **Highest hit rate** (40-60%) among strategies that require no new models or training data.
2. **Lowest complexity**  -  reuses existing graph queries and fact loading.
3. **No cold start**  -  the knowledge graph is already populated by extraction.
4. **Natural fit**  -  the existing Tier 3 search already does graph expansion reactively; this just moves it earlier.
5. **Proves the architecture**  -  the SpeculativeCache and merge logic built for Strategy A are reusable for all other strategies.

### Implementation order

1. **Phase 1 (MVP):** Strategy A with feature flag, conservative defaults (1-hop only, 20-fact cache, 0.8 score penalty). Ship behind flag, instrument heavily.
2. **Phase 2 (Validate):** Run A/B comparison for 2-4 weeks. Measure hit rate, latency delta, and whether injected speculative facts appear in LLM outputs.
3. **Phase 3 (Expand):** If Phase 2 shows >25% hit rate, add Strategy B (trajectory vector) for sessions where graph coverage is sparse.
4. **Phase 4 (Learn):** If user patterns become trackable, add Strategy D as a long-term enhancement.

### Configuration defaults

```toml
[recall.predictive]
enabled = false                    # Opt-in initially
strategy = "graph_neighborhood"    # MVP strategy
max_cache_facts = 20               # Bounded cache size
cache_ttl_turns = 2                # Expire after 2 unused turns
cosine_gate = 0.5                  # Minimum similarity to use cache
score_penalty = 0.8                # Speculative score multiplier
max_hops = 1                       # Conservative neighborhood depth
max_prefetch_ms = 50               # Wall-clock budget for speculative work
```

---

## Gotchas

1. **Extraction timing.** Graph neighborhood pre-loading depends on extraction having identified entities from the current turn. Extraction runs in the background and may not complete before the user's next message arrives (especially for short turn gaps). The speculative cache must handle the case where no entities are available yet  -  fall back to no-op rather than blocking.

2. **Hub entity explosion.** High-PageRank entities (e.g., the user's own name, a project name) can have hundreds of 1-hop neighbors. The `max_prefetch_ms` budget and a PageRank-based neighbor limit (skip neighbors with PageRank < 0.01) prevent this from becoming a latency spike.

3. **Score penalty tuning.** The 0.8 multiplier is a guess. Too aggressive and speculative results never surface; too lenient and they displace better fresh results. This needs empirical tuning against real conversations. Log the penalty-adjusted vs raw scores to enable offline analysis.

4. **Interaction with iterative recall.** The existing `run_iterative()` path does terminology discovery and a second search cycle. Speculative results should be merged after the iterative cycles complete, not before  -  otherwise the speculative cache could prevent terminology discovery from finding novel terms (the system would think it already has enough results).

5. **Cache coherence with fact updates.** If a fact is superseded or forgotten between the speculative fetch and the next recall, the cached version is stale. Check `is_forgotten` and `superseded_by` at merge time, not at cache time.

6. **Multi-nous contention.** In multi-agent scenarios, each `NousActor` has its own speculative cache. This is correct (agents have different knowledge scopes) but means total memory scales with active agent count. At 20KB per agent, this is negligible for typical deployments (<100 agents).

7. **The 5 static factors.** Today, only `vector_similarity` is computed from the actual query  -  the other 5 factors (decay, relevance, epistemic tier, proximity, frequency) use config defaults or per-fact static values (see `nous/src/recall.rs:470-493`). This means speculative scoring is nearly identical to fresh scoring for those factors. If the scoring model evolves to make more factors query-dependent, speculative pre-scoring becomes less reliable and the cache-then-rescore architecture becomes more important.

---

## Observations

- **Debt:** `nous/src/recall.rs:470-493`  -  Only `vector_similarity` is query-computed; the other 5 factor scores are config defaults, not actual per-fact values from mneme. The `decay`, `epistemic_tier`, and `access_frequency` scores should come from the fact's actual metadata (age, tier, access_count) via `RecallEngine::score_decay()` etc. This was likely a simplification during initial integration.
- **Idea:** The `SpeculativeCache` infrastructure could also be used for "recall pinning"  -  letting users explicitly pin facts that should always appear in recall, bypassing scoring entirely.
- **Doc gap:** No documentation exists for the 6-factor scoring model's weight rationale. The weights (0.35/0.20/0.15/0.15/0.10/0.05) appear to be hand-tuned. An ablation study comparing weight configurations would validate or improve them.

---

## References

- `crates/nous/src/recall.rs`  -  RecallStage, iterative recall, scoring, formatting
- `crates/nous/src/pipeline.rs`  -  6-stage turn pipeline, recall invocation
- `crates/nous/src/actor/background.rs`  -  extraction trigger, skill capture
- `crates/mneme/src/recall.rs`  -  RecallEngine, 6-factor scoring, FSRS decay
- `crates/mneme/src/knowledge_store/search.rs`  -  HNSW, BM25, hybrid, tiered search
- `crates/mneme/src/knowledge_store/marshal.rs`  -  HybridQuery building, RRF merge
- `crates/mneme/src/graph_intelligence.rs`  -  PageRank, Louvain, proximity boosting
- `crates/mneme/src/query_rewrite.rs`  -  LLM query rewriting, tiered search config
- `crates/mneme/src/hnsw_index.rs`  -  HNSW vector index wrapper
- `crates/mneme/src/knowledge.rs`  -  Fact, Entity, EpistemicTier types
- `crates/nous/src/config.rs`  -  NousConfig, RecallConfig, PipelineConfig
- `crates/nous/src/distillation.rs`  -  distillation triggers (context/message thresholds)
