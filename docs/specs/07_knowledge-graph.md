# Spec: Knowledge Graph — Performance, Utility, and Integration

**Status:** Phases 1a-1b, 2a-2b, 3a-3e done. Phases 2c, 3f remaining.  
**Author:** Syn  
**Date:** 2026-02-19  

---

## Problem

The knowledge graph (Neo4j + Qdrant via Mem0 sidecar) is the most architecturally ambitious piece of Aletheia — and the least utilized. It stores entity relationships, extracted memories, and conversation episodes, but:

### Performance

- **Recall is slow.** The `recallMemories` function in `nous/recall.ts` has a 3-second timeout, and frequently *hits* it. Graph-enhanced search (`/graph_enhanced_search`) compounds latency: it queries Qdrant for vector matches, then Neo4j for graph context, then merges. On a modest server, this regularly exceeds 3 seconds.
- **Neo4j is heavy.** It consumes ~500MB of disk and meaningful RAM for what is currently a small graph. The query patterns are simple (1-2 hop traversals, entity lookups) — not the complex graph algorithms Neo4j is designed for.
- **Startup cost.** Neo4j + Qdrant via Docker add 30+ seconds to system startup and consume resources continuously.

### Utility

- **Recall quality is inconsistent.** The graph returns memories by vector similarity + graph proximity, but relevance is hit-or-miss. A question about leather working might surface a memory about truck maintenance because both mention "tools." The 0.75 minimum score filter helps but doesn't solve semantic drift.
- **No user visibility.** The graph view in the webchat is a force-directed visualization that looks cool but offers no actionable insight. You can't search it, can't edit it, can't see what the system "knows" about a topic.
- **Extraction quality varies.** The distillation extraction pass pulls facts, decisions, and entities, but the quality depends on the conversation. Tool-heavy sessions produce noisy extractions ("User ran git status" is a fact but not useful long-term).
- **No feedback loop.** There's no way to tell the system "this memory is wrong" or "this connection doesn't exist." Errors in the graph compound over time.

### Integration

- **Graph is passive.** Memories are recalled at the start of each turn via pre-flight search, but the agent can't actively query the graph during a turn. It's "here's what I found" not "let me look that up."
- **Cross-agent memory is poorly differentiated.** All agents share the same Qdrant collection. Syn's operational memories about pipeline architecture mix with Demiurge's leather crafting knowledge. The `agent_id` filter helps but doesn't create meaningful separation.

---

## Design

### Phase 1: Performance — Make It Fast

#### Replace graph-enhanced search with simpler retrieval

The current flow: vector search → graph traversal → merge → rank. The graph traversal rarely adds enough value to justify 2-3x latency.

**New flow:** 
1. **Primary:** Qdrant vector search only (fast, ~200ms)
2. **Optional enrichment:** If the top results mention entities with graph relationships, fetch 1-hop connections as supplementary context. Do this asynchronously — don't block the turn.

```typescript
export async function recallMemories(messageText: string, nousId: string): Promise<RecallResult> {
  // Fast path: vector search only
  const hits = await fetchVectorSearch(query, nousId, limit);
  
  // Optional: async graph enrichment (doesn't block turn)
  if (hits.length > 0) {
    enrichWithGraphContext(hits, nousId).catch(() => {}); // fire-and-forget
  }
  
  return buildRecallBlock(hits);
}
```

**Expected improvement:** Recall latency drops from 2-3s (with frequent timeouts) to ~200-500ms.

#### Evaluate Neo4j replacement

For our usage patterns (entity storage, simple relationship queries, 1-2 hop traversals), Neo4j is overkill. Evaluate:

- **SQLite with a relationships table** — Zero additional services. Entities and relationships stored alongside sessions. Fast for simple lookups. Loses complex graph queries but we don't use them.
- **Qdrant metadata** — Store entity relationships as metadata on vector entries. Eliminates Neo4j entirely. Limited query flexibility.
- **Keep Neo4j but optimize** — Reduce memory allocation, add query caching, precompute common traversals. Keeps graph capabilities for future use.

**Recommendation:** Start with making Neo4j optional (graceful degradation when it's not running) ✅, then evaluate SQLite replacement based on actual query patterns. Don't rip out Neo4j prematurely — the graph capabilities may matter as the system matures.

#### Reduce sidecar overhead

The Python Mem0 sidecar adds latency and a failure point. For the vector search path, consider calling Qdrant directly from the runtime (Node.js Qdrant client) instead of proxying through the sidecar. The sidecar remains for graph operations and memory management but isn't in the hot path for recall.

### Phase 2: Utility — Make It Useful

#### Improve extraction quality

The current extraction prompt treats all conversations equally. Add context-aware filtering:

```typescript
const EXTRACTION_PROMPT = `...
## Context-Dependent Filtering
- For TOOL-HEAVY turns: extract the CONCLUSIONS and OUTCOMES, not the tool invocations
  - Bad: "User ran grep to search for imports"
  - Good: "The auth module has 6 unused exports"
- For DISCUSSION turns: extract the OPINIONS, PREFERENCES, and DECISIONS
  - Bad: "User and agent discussed leather options"
  - Good: "User prefers chrome-tanned leather for belts due to durability, rejects veg-tan for this use case"
- For PLANNING turns: extract the PLANS, TIMELINES, and COMMITMENTS
  - Bad: "User asked about schedule"
  - Good: "MBA final project due March 15, needs 3 weeks of work, starting after midterms"
...`;
```

#### Memory confidence and decay

Memories should have a confidence score that changes over time:

- **Reinforced memories** (referenced again in conversation) → confidence increases
- **Contradicted memories** (new information conflicts) → old memory flagged, new one created
- **Aging memories** (not referenced for 90+ days) → confidence decays toward retrieval threshold
- **Corrected memories** (user explicitly says "that's wrong") → marked as corrected with the correction

This prevents the graph from accumulating stale facts that were true once but aren't anymore.

#### Active graph querying

Give agents a `memory_query` tool that lets them actively search the graph during a turn:

```typescript
// Tool definition
{
  name: "memory_query",
  description: "Search long-term memory for facts, preferences, and relationships from past conversations.",
  input: {
    query: "What does the user prefer for leather belt construction?",
  }
}
```

This already exists as `mem0_search` — but it's an agent tool, not integrated into the recall flow. The improvement is making the tool result richer: return not just the memories but their confidence scores, when they were last confirmed, and any related entities.

#### Graph UI overhaul

Replace the force-directed visualization with a searchable, browsable knowledge base:

- **Search bar** — query memories by text, entity, or topic
- **Entity cards** — click an entity to see all facts, relationships, and conversations mentioning it
- **Timeline view** — when was a fact learned, reinforced, or contradicted?
- **Edit/delete** — correct wrong memories, delete irrelevant ones, merge duplicates
- **Confidence indicators** — visual markers for high-confidence vs. uncertain memories

This turns the graph from a demo into a tool.

### Phase 3: Integration — Make It Smart

#### Domain-scoped memory

Instead of all agents sharing one flat memory pool, scope memories by domain:

| Domain | Agents | Content |
|--------|--------|---------|
| `personal` | All (read), Syn (write) | Health, preferences, relationships, schedule |
| `craft` | Demiurge (read/write) | Leather techniques, tool preferences, project specs |
| `home` | Syl (read/write) | Family logistics, household, calendar |
| `truck` | Akron (read/write) | Vehicle maintenance, radio, preparedness |
| `work` | Syn (read/write) | Summus, dashboards, SQL, colleagues |
| `system` | Syn (read/write) | Aletheia architecture, infrastructure decisions |

Cross-domain queries are allowed but explicitly scoped. When Syn recalls memories, it searches `personal` + `work` + `system` by default. When Demiurge recalls, it searches `personal` + `craft`.

#### Thread-aware recall

The current recall searches based on message text only. Improve by incorporating thread context:

- **Thread summary** (already implemented in thread model) → use as additional search context
- **Recent entities** — if the last 5 messages mention "leather" and "belt", boost memories about leather crafting
- **Agent identity** — each agent's recall should be weighted toward their domain

#### Proactive memory surfacing

Instead of only recalling on user message, surface memories when they become relevant:

- Agent mentions an entity → check if there are contradicting memories about that entity
- User asks a question → check if the answer was previously discussed (prevent the agent from re-researching)
- Distillation extracts a new fact → check if it contradicts an existing memory

---

## Implementation Order

| Phase | Effort | Impact |
|-------|--------|--------|
| **1a: Vector-only fast path** | Small | Recall drops from 3s → 500ms |
| **1b: Neo4j optional mode** | Medium | System works without Neo4j running |
| **2a: Extraction prompt improvement** ✅ | Small | Better memory quality |
| **2b: Memory confidence/decay** ✅ | Medium | ✅ Done — Neo4j access/decay weighting in sidecar search, penalty for decayed, boost for accessed (PR #75) |
| **2c: Graph UI search + edit** | Medium | User can see and correct the knowledge base |
| **3a: Domain-scoped memory** ✅ | Medium | ✅ Done — `domains` field on NousConfig, sidecar filters by domain metadata (PR #75) |
| **3b: Thread-aware recall** | Small | Better relevance |
| **3c: MMR diversity re-ranking (F-10)** | Small | Post-processing on search results — Jaccard overlap penalty to reduce redundant recalls |
| **3d: Sufficiency gates (F-14)** | Small | Tiered retrieval — category summaries first, full items only if top results insufficient |
| **3e: Self-editing memory tools (F-22)** | Small | `memory_update`/`memory_forget` tools — agents directly modify their own knowledge |
| **3f: Tool memory / usage stats (F-31)** | Small | Track success/failure rates per tool per agent — inform tool selection and detect degradation |

---

## Success Criteria

- **Recall latency p95 < 500ms** (currently frequently timing out at 3s)
- **Zero timeout warnings** in a typical session
- **User can search, view, and correct** memories through the UI
- **Extraction quality:** manual review of 20 distillations shows <10% noise (currently ~30-40%)
- **Memory confidence** decays appropriately — facts from 6 months ago that haven't been reinforced score lower than recent facts
