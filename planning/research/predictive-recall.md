# R715: Predictive Recall System

## Question

Current recall is reactive: it retrieves memories in response to the current message. Can we anticipate what information the agent will need based on conversation trajectory, pre-fetching relevant memories before they are explicitly needed? What strategies are viable, what are the risks, and what is the minimum viable approach?

## Findings

### Current Recall Pipeline

The recall pipeline runs as stage 2 of 6 in the nous pipeline (`crates/nous/src/pipeline.rs:504-574`), after context assembly but before history retrieval. It has a 15-second timeout budget and degrades gracefully (skips recall if timeout exceeded).

**Trigger:** Every user message triggers a full recall cycle. There is no predictive, speculative, or background recall.

**Signals (6-factor scoring engine in `crates/mneme/src/recall.rs`):**

| Signal | Weight | Source |
|--------|--------|--------|
| Vector similarity (cosine) | 0.35 | HNSW index, 384-dim BGE-small embeddings |
| FSRS decay (spaced repetition) | 0.20 | Power-law decay based on access recency and stability |
| Relevance (ownership) | 0.15 | Nous-id matching (own vs shared vs foreign) |
| Epistemic tier | 0.15 | Verified/Inferred/Assumed, boosted by PageRank |
| Relationship proximity | 0.10 | Graph hop distance, floored by Louvain community |
| Access frequency | 0.05 | Logarithmic access count, supersession chain boost |

**Latency breakdown (typical):**

| Stage | Latency |
|-------|---------|
| Embedding (candle, CPU) | 10-100 ms |
| HNSW search (100K vectors) | < 1 ms |
| BM25 fallback | 1-5 ms |
| Scoring and ranking | < 1 ms |
| Formatting and injection | < 1 ms |
| **Total recall** | **50-200 ms** |

Embedding dominates recall latency. HNSW search is sub-millisecond even at 100K vectors (384 dimensions, m=16, ef=50).

**Existing optimizations:**
- HNSW index held in memory (~160 MB for 100K vectors)
- Graph scores (PageRank, Louvain) pre-computed and cached in `graph_scores` relation
- Iterative recall (disabled by default): 2-cycle refinement with query expansion
- Tiered search escalation: fast path, then enhanced (query rewriting), then graph-enhanced
- LLM prompt caching at the provider level for system prompt sections

**Notable gaps:**
- Nous scoring layer uses static config weights, not actual per-fact decay/access/proximity values from the knowledge store
- No background pre-fetch or speculative embedding
- No conversation trajectory analysis
- Iterative recall and query rewriting disabled by default

### Predictive Recall Strategies

#### Strategy 1: Graph Neighborhood Pre-Loading

**Mechanism:** After recall returns results, identify the entities mentioned and pre-load their 1-2 hop graph neighborhoods into a warm cache. When the next message arrives, check the cache before running full recall.

**Pros:**
- Low implementation complexity. The graph traversal primitives already exist in `crates/mneme/src/graph_intelligence.rs`
- High hit rate for conversations that stay on-topic: if discussing entity X, related entities Y and Z are often needed next
- Cache is small (tens of facts per neighborhood) and cheap to maintain
- No LLM call required

**Cons:**
- Useless for topic shifts. Cache invalidation on conversation pivot wastes the pre-fetch
- Graph density varies. Sparse regions yield nothing; dense regions yield too much

**Estimated hit rate:** 40-60% for topic-continuous conversations, <10% for exploratory conversations.

#### Strategy 2: Embedding Pre-Computation During Idle Time

**Mechanism:** While the user is typing (detected via SSE heartbeat gap or typing indicator), pre-compute embeddings for likely follow-up queries derived from the last assistant response.

**Implementation approach:**
1. Extract key noun phrases and entity references from the last assistant response
2. Generate 2-3 candidate query embeddings in the background
3. Run HNSW search for each, cache the top-K results
4. When the actual message arrives, check if any cached embedding is within a cosine threshold of the real query embedding; if so, skip the full recall path

**Pros:**
- Hides the 10-100 ms embedding latency entirely for predicted queries
- HNSW search is already sub-millisecond, so the bottleneck is embedding
- Candidate generation is deterministic (no LLM needed): noun phrase extraction suffices

**Cons:**
- Requires a "user is typing" signal. Current architecture may not expose this
- Embedding 2-3 variants triples the embedding compute during idle time
- Wasted compute if the user asks something unrelated
- Cache coherence: pre-fetched results may become stale if extraction runs concurrently

**Estimated latency savings:** 50-80 ms per message when prediction hits (eliminates embedding step).

#### Strategy 3: N-Gram Topic Trajectory Prediction

**Mechanism:** Maintain a sliding window of topic tags extracted from recent messages. Use n-gram frequency analysis to predict likely next topics and pre-fetch their associated facts.

**Implementation:**
1. After each message, extract topic tags (entity types, fact categories, keywords)
2. Maintain a frequency table of bigram topic transitions (topic A followed by topic B)
3. When topic A is active, pre-fetch facts tagged with top-3 predicted next topics
4. Update transition probabilities with exponential moving average

**Pros:**
- Learns user-specific patterns over time (recurring topic sequences)
- Lightweight computation (hash table lookups, no embedding)
- Can warm the cache minutes before the user even asks

**Cons:**
- Requires meaningful topic extraction (a prerequisite system that doesn't exist yet)
- Cold start: no predictions until sufficient conversation history accumulates
- Topic transitions may be too sparse to build reliable n-gram models for most users
- Storage overhead for transition tables per-user

**Estimated hit rate:** 20-35% initially, potentially 50%+ after weeks of usage with consistent patterns.

#### Strategy 4: Last-N Related Conversations Heuristic

**Mechanism:** When a conversation starts, pre-fetch the recall results from the last N conversations with the same nous. Rationale: users often resume or continue previous work.

**Implementation:**
1. On session creation, retrieve the last 3-5 session summaries
2. Run recall for each summary's topic embedding
3. Cache the union of results, deduplicated by source_id
4. Inject as "background context" at lower priority than live recall

**Pros:**
- Simple to implement. Session history and embeddings already exist
- High value for "continuing yesterday's work" patterns
- No real-time prediction needed; runs once at session start

**Cons:**
- Wastes tokens if the user is starting a new topic
- 3-5 recall cycles at session start adds 150-1000 ms to first-message latency
- Stale results from old sessions may confuse more than help
- Token budget pressure: pre-fetched context competes with live recall budget (2000 tokens default)

**Estimated value:** High for returning users with consistent workflows, low for ad-hoc usage.

#### Strategy 5: User Pattern Learning (Time-Based)

**Mechanism:** Track what topics a user accesses by time-of-day and day-of-week. Pre-fetch accordingly.

**Pros:**
- Captures "morning standup topics" vs "afternoon deep-work topics" patterns
- Fully automated after training period

**Cons:**
- Requires weeks of data to detect patterns
- Patterns are fragile: one schedule change invalidates predictions
- Privacy concern: temporal usage patterns are sensitive metadata
- Implementation complexity disproportionate to value

**Recommendation:** Defer. This is a v3 optimization, not an MVP concern.

### Negative Findings

**LLM-based trajectory prediction is not viable for MVP.** Using an LLM call to predict "what will the user ask next" adds 500-2000 ms latency and costs tokens. The prediction must be faster than the recall it replaces to provide value.

**Full conversation replay for prediction is wasteful.** Re-embedding the entire conversation to find trajectory patterns scales poorly (O(n) in conversation length) and the signal-to-noise ratio drops as conversations grow.

**Pre-fetching all graph neighborhoods is too expensive.** A densely connected knowledge graph could yield thousands of facts in 2-hop neighborhoods. Without selectivity, this becomes a memory pressure problem, not a performance optimization.

### Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Wasted compute on wrong predictions | Increased CPU/embedding cost | High (40-60% miss rate) | Budget cap: max 3 speculative embeddings per turn. Monitor miss rate; disable if >80% |
| Stale predictions after topic shift | Irrelevant results injected | Medium | TTL on cached predictions (expire after 1 turn if unused). Clear cache on detected topic shift (cosine distance > threshold between consecutive messages) |
| Memory pressure from cached results | OOM or GC pressure | Low | Bounded LRU cache: max 50 cached facts. Evict on turn boundary |
| Token budget competition | Live recall crowded out by speculative results | Medium | Separate token budgets: speculative results capped at 500 tokens, live recall retains full 2000 token budget. Live results always take priority |
| Complexity in recall pipeline | Harder to debug, more failure modes | Medium | Feature-gated behind config flag. Metrics on prediction hit rate, latency delta, cache size. Disable path must be zero-cost |

### Effort Estimates

| Strategy | Effort | Value | Recommended Phase |
|----------|--------|-------|-------------------|
| Graph neighborhood pre-loading | 3-5 days | Medium | MVP (Phase 1) |
| Last-N sessions heuristic | 2-3 days | Medium-High | MVP (Phase 1) |
| Idle-time embedding pre-computation | 5-8 days | High | Phase 2 (requires typing signal) |
| N-gram topic trajectory | 8-12 days | Medium | Phase 3 (requires topic extraction) |
| Time-based user patterns | 10-15 days | Low | Defer |

## Recommendations

### Minimum Viable Approach: Phase 1

Combine **graph neighborhood pre-loading** and **last-N sessions heuristic** as the MVP. Together they cover the two most common conversation patterns (topic continuity and session resumption) with approximately 5-8 days of implementation effort.

**Implementation plan:**

1. **Add a `PredictiveCache` struct to mneme** (new module: `crates/mneme/src/predictive_cache.rs`)
   - Bounded LRU cache keyed by source_id
   - Max 50 entries, TTL of 2 turns (evict if not accessed within 2 recall cycles)
   - Thread-safe (`Arc<RwLock<...>>` since reads dominate)

2. **Graph pre-loading hook** (in `crates/mneme/src/recall.rs`, after scoring)
   - After `RecallEngine::rank()` returns, extract entity IDs from top-5 results
   - Spawn a background task to load 1-hop neighborhoods for those entities
   - Insert results into `PredictiveCache` with lower priority score (0.8x multiplier)

3. **Session-start pre-fetch** (in `crates/nous/src/pipeline.rs`, during context stage)
   - On first message of a session, retrieve last 3 session summaries from mneme
   - Run lightweight recall (BM25 only, no embedding) for each summary
   - Populate `PredictiveCache`

4. **Cache consultation in recall** (in `crates/nous/src/recall.rs`, before embedding)
   - Before running full recall, check `PredictiveCache` for matches
   - Merge cached results with live results, de-duplicating by source_id
   - Live results always win on score ties

5. **Metrics** (in `crates/mneme/src/predictive_cache.rs`)
   - Track: cache_hits, cache_misses, cache_size, evictions, prediction_accuracy
   - Expose via tracing spans for observability

**Config additions** (in taxis):
```
[recall.predictive]
enabled = false          # Feature-gated, opt-in
graph_preload = true     # Pre-load graph neighborhoods
session_preload = true   # Pre-load from recent sessions
max_cached_facts = 50    # LRU capacity
ttl_turns = 2            # Evict unused predictions after N turns
score_discount = 0.8     # Predicted results scored at 80% of live results
```

### Phase 2: Idle-Time Embedding

Requires upstream work to expose a "user is typing" signal through the SSE connection. Once available:

1. Extract noun phrases from last assistant response
2. Pre-compute 2-3 embeddings during typing gap
3. Cache HNSW results for each
4. On message arrival, check if real query embedding is within cosine distance 0.15 of any cached embedding; if so, use cached results directly

### Phase 3: Topic Trajectory

Requires a topic extraction system (potentially from dianoia's entity extraction). Build a lightweight Markov chain of topic transitions per-user and pre-fetch predicted next-topic facts.

## Gotchas

1. **The nous scoring layer doesn't use actual per-fact signals.** `crates/nous/src/recall.rs:470-493` builds candidates with static config weights rather than querying actual decay, access count, and proximity data from mneme. Predictive cache results will have the same limitation. Fixing this is a separate concern but would improve both live and predictive recall quality.

2. **Graph dirty flag mechanism is incomplete.** `GraphDirtyFlag` in `crates/mneme/src/graph_intelligence.rs` exists but the invalidation trigger is not fully wired. Pre-loaded neighborhoods could serve stale graph scores. Ensure graph score recomputation runs before neighborhood pre-loading, or accept eventual consistency.

3. **Token budget interaction.** The recall stage has a 2000-token budget. Predictive results must not crowd out live results. The score discount (0.8x) and separate budget cap (500 tokens for speculative) prevent this, but the interaction needs integration testing.

4. **Iterative recall interaction.** If iterative recall is enabled (currently disabled by default), the 2-cycle refinement may conflict with predictive cache hits. The cache should be consulted before cycle 1 but not between cycles.

5. **Embedding provider mock mode.** When the embedding provider is mock (test/dev), predictive embedding pre-computation is meaningless. The feature should no-op when `EmbeddingProvider` is mock.

6. **CozoDB query cost.** Graph neighborhood queries via Datalog are not free. Pre-loading 5 neighborhoods per turn adds 5 CozoDB queries. Profile to ensure this stays under 10 ms total.

## References

- `crates/mneme/src/recall.rs` - 6-factor scoring engine, weights, RecallEngine
- `crates/mneme/src/knowledge_store/search.rs` - Vector search, BM25, hybrid search, tiered escalation
- `crates/mneme/src/hnsw_index.rs` - HNSW index configuration (384-dim, m=16, ef=200)
- `crates/mneme/src/graph_intelligence.rs` - PageRank, Louvain community, graph signal augmentation
- `crates/mneme/src/query_rewrite.rs` - Query variant generation, tiered search
- `crates/mneme/src/embedding.rs` - Embedding providers (candle BGE-small, mock, voyage placeholder)
- `crates/mneme/src/knowledge.rs` - FSRS decay model, fact type stability constants
- `crates/nous/src/recall.rs` - Recall stage orchestration, iterative recall, candidate building
- `crates/nous/src/pipeline.rs:504-574` - Pipeline stage invocation, timeout handling
- `crates/nous/src/config.rs` - Stage timeouts, recall parameters
- `crates/nous/src/actor/background.rs` - Background extraction task spawning
- FSRS algorithm: Open Spaced Repetition project (power-law variant used here)
- HNSW: Malkov & Yashunin, "Efficient and robust approximate nearest neighbor using Hierarchical Navigable Small World graphs" (2018)
