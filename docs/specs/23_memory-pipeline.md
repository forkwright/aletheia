# Spec 23: Memory Pipeline — Extraction, Quality, and Retrieval

**Status:** Active — Phases 1-3 done, 4+6 partial, 5 pending  
**Author:** Syn  
**Date:** 2026-02-21  
**Spec:** 23  

---

## Problem

The memory system has three layers (Qdrant vectors, Neo4j graph, Mem0 sidecar) but the pipeline connecting them is broken, noisy, and underutilized. Tonight's deep audit revealed the extent:

### 1. No after-turn extraction pipeline

The `MemoryFlushTarget` interface exists in `distillation/hooks.ts` with a clean `addMemories()` contract. The distillation pipeline (`pipeline.ts`) accepts an optional `memoryTarget` parameter. The reflection cron (`reflection-cron.ts`) also accepts one.

**Neither is wired.** `aletheia.ts` never creates or passes a `MemoryFlushTarget` implementation. The distillation pipeline extracts facts via Haiku (`extract.ts`) but drops them — they go into the workspace flush (`workspace-flush.ts`) as text files, never into the vector store.

The 607 `source: after_turn` entries in Qdrant are orphans — created by an earlier code path that no longer exists. New conversations produce zero persistent memories until an agent manually calls `mem0_search` (which only searches, never writes) or someone manually hits the sidecar `/add` endpoint.

**Impact:** The system forgets everything between distillations. A 2-hour conversation generates zero searchable memories. The entire recall system (`recall.ts`) searches a stale corpus.

### 2. Extraction quality is poor

When memories do get created (via sidecar `/add` → Mem0 internal extraction), the quality is bad:

- **Qdrant noise (pre-audit):** 213 entries like "Uses grep", "Works with a system called Aletheia", "Familiar with confabulation guards." Generic observations with no durable value. This is ~13% of the corpus.
- **Neo4j garbage (pre-audit):** 81% of relationships were generic `RELATES_TO` (zero semantic signal). 109 stopword entities ("The", "If", "That"). Entities like `cpu` accumulated 118 fake relationships.
- **Root cause:** Mem0 uses its own Haiku extraction prompt internally. Our `extract.ts` has a good context-aware prompt (tool-heavy → conclusions, discussion → preferences, debugging → root causes) but it's disconnected from the memory store. The sidecar's `/add` endpoint passes text to `mem.add()` which uses Mem0's default extraction — we don't control what it extracts.

**Impact:** Recall returns noise. A query about "truck steering" might get 0.83 relevance, but "Cody preferences" gets 0.73 with "User knows someone named Cody" as the top hit. The signal-to-noise ratio degrades with every extraction.

### 3. Neo4j is structurally limited

After deep cleaning (3,753 → 1,566 nodes, 7,533 → 1,093 relationships), the graph is now typed (`USES`, `OWNS`, `INTERESTED_IN`, etc.) but architecturally limited:

- **All relationships route through `__User__`** — `Cody → USES → signal`, `Cody → OWNS → 1997_ram_2500`. No entity-to-entity relationships exist (`1997_ram_2500 → HAS_PART → steering_box` doesn't exist).
- **Entity resolution is nonexistent** — `aletheia_runtime`, `aletheia`, `aletheia_system` are three separate nodes for the same thing.
- **Extraction creates entities opportunistically** — heuristic extraction (`_extract_entities_for_episode`) capitalizes words and calls them entities. No schema, no validation, no linking to existing entities.

**Impact:** Graph-enhanced search adds 1-2s latency for minimal value. The graph neighbors it finds are random entity names, not meaningful context. Vector-only search is faster and produces comparable or better results.

### 4. No memory lifecycle

Memories are write-only. There's no:
- **Contradiction detection** — new facts don't check against existing facts
- **Confidence decay** — facts from January have the same weight as facts from today  
- **Deduplication across sources** — the same fact extracted during distillation and during reflection creates two entries
- **User correction path** — `fact_retract` tool exists but requires knowing the memory ID, and there's no browseable memory UI

Spec 07 Phase 2b added access/decay weighting in the sidecar search (PR #75), but it's cosmetic — the underlying data has no confidence scores, no access tracking, and no contradiction flags.

---

## Design

### Principles

1. **Wire what exists before building new things.** The extraction prompt, the flush interface, the sidecar — the pieces exist. They need to be connected.
2. **Control extraction quality at the source.** Don't let Mem0's default prompt extract. Use our own extraction and store the results directly in Qdrant.
3. **Vector search is the primary path.** Neo4j is supplementary. Don't block turns on graph queries.
4. **Memory should be fresh.** After-turn extraction means recall reflects the last conversation, not last week's distillation.
5. **Less is more.** 200 high-quality memories beat 2,000 noisy ones. Aggressive filtering > permissive extraction.

### Architecture

Current flow (broken):
```
Turn ends → nothing happens
Distillation → extract.ts extracts facts → workspace flush (text files) → memoryTarget not wired → sidecar never called
Reflection → finds patterns → memoryTarget not wired → sidecar never called
Recall → searches stale Qdrant corpus → returns noise
```

Target flow:
```
Turn ends → finalize.ts extracts key facts (lightweight, Haiku) → sidecar /add_direct (bypass Mem0 re-extraction) → Qdrant
Distillation → extract.ts extracts facts → flushToMemory → sidecar /add_direct → Qdrant (deduped against existing)
Reflection → deep pattern extraction → flushToMemory → sidecar /add_direct → Qdrant
Recall → vector search with MMR diversity → return ranked, deduped, scored memories
```

Key change: **`/add_direct`** — a new sidecar endpoint that stores pre-extracted facts as vectors WITHOUT re-extracting through Mem0's LLM. Our extraction is better; Mem0's extraction adds latency, cost, and noise.

---

## Phases

### Phase 1: Wire the Extraction Pipeline

**Scope:** Connect distillation extraction to the memory sidecar. This is the highest-impact, lowest-effort fix — the code exists on both sides, it just needs a bridge.

**Changes:**

1. **`infrastructure/runtime/src/aletheia.ts`** — Create a `MemoryFlushTarget` implementation:
```typescript
const memoryTarget: MemoryFlushTarget = {
  async addMemories(agentId: string, memories: string[]): Promise<{ added: number; errors: number }> {
    const url = `${process.env.ALETHEIA_MEMORY_URL ?? 'http://127.0.0.1:8230'}/add_batch`;
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        texts: memories,
        user_id: process.env.ALETHEIA_MEMORY_USER ?? 'default',
        agent_id: agentId,
        source: 'distillation',
      }),
    });
    const data = await res.json();
    return { added: data.added ?? 0, errors: data.errors ?? 0 };
  }
};
```
   Pass `memoryTarget` to `distillSession()` in `manager.ts` and to `runNightlyReflection()` / `runWeeklyReflection()` in the cron registration.

2. **`infrastructure/memory/sidecar/aletheia_memory/routes.py`** — New `/add_batch` endpoint:
   - Accepts `{ texts: string[], user_id, agent_id, source }`
   - Embeds each text via Ollama (same as backfill)
   - Dedup check against existing vectors (cosine > 0.90 = skip)
   - Stores directly in Qdrant — **no Mem0 LLM extraction**
   - Returns `{ added: N, skipped: N, errors: N }`

3. **`infrastructure/memory/sidecar/aletheia_memory/routes.py`** — New `/add_direct` endpoint:
   - Single-item version of `/add_batch`
   - Same bypass: embed + store, no Mem0 extraction
   - Used by after-turn lightweight extraction (Phase 2)

**Acceptance Criteria:**
- [ ] Distillation pipeline flushes extracted facts to Qdrant via sidecar
- [ ] Reflection pipeline flushes findings to Qdrant via sidecar
- [ ] New memories appear in Qdrant within 30s of distillation completing
- [ ] Dedup prevents duplicate facts across distillation runs (cosine > 0.90)
- [ ] `source: distillation` and `source: reflection` metadata on new entries
- [ ] Zero impact on turn latency (flush is async, post-distillation)

**Testing:**
- Integration test: trigger distillation → verify new Qdrant entries
- Unit test: `MemoryFlushTarget.addMemories` → verify HTTP call shape
- Dedup test: add same fact twice → second is skipped

**LOC estimate:** ~150 (runtime wiring) + ~120 (sidecar endpoints) = **~270**

---

### Phase 2: After-Turn Lightweight Extraction

**Scope:** Extract 0-3 key facts from each turn immediately after it completes. Keeps the memory corpus fresh without waiting for distillation.

**Changes:**

1. **`infrastructure/runtime/src/nous/pipeline/stages/finalize.ts`** — Add memory extraction alongside existing working-state and skill extraction:
```typescript
// After-turn memory extraction — lightweight, non-blocking
if (memoryTarget && outcome.text.length > 100) {
  extractTurnFacts(services.router, outcome.text, toolSummary, turnFactsModel)
    .then(facts => {
      if (facts.length > 0) {
        memoryTarget.addDirect(nousId, facts);
      }
    })
    .catch(err => log.debug(`Turn fact extraction failed: ${err.message}`));
}
```

2. **`infrastructure/runtime/src/nous/turn-facts.ts`** (new) — Lightweight extraction prompt:
   - Input: assistant response text + tool summary (not full conversation)
   - Output: 0-3 facts, each ≤100 chars
   - Model: Haiku (same as working-state extraction)
   - Prompt focuses on: decisions made, preferences stated, facts learned, corrections applied
   - Hard filter: skip if turn is <100 chars, skip tool-only turns, skip greetings
   - Budget: ~500 input tokens, ~200 output tokens per turn (~$0.0001 on Haiku)

3. **`infrastructure/runtime/src/nous/manager.ts`** — Thread `memoryTarget` through to the pipeline runner

**Acceptance Criteria:**
- [ ] 0-3 facts extracted per qualifying turn, stored in Qdrant within 5s
- [ ] Non-qualifying turns (greetings, tool-only, short) produce zero extractions
- [ ] Source metadata: `source: turn, sessionId, turnId`
- [ ] Total added latency to turn: <200ms (extraction is async, doesn't block response)
- [ ] Cost per turn: <$0.001 on Haiku

**Testing:**
- Unit test: mock turn with decision → extracts fact
- Unit test: greeting turn → extracts nothing
- Integration test: send message → verify Qdrant entry within 5s

**LOC estimate:** ~200 (new turn-facts module + prompt) + ~50 (finalize.ts wiring) + ~30 (manager threading) = **~280**

---

### Phase 3: Extraction Quality — Our Prompt, Not Mem0's

**Scope:** Replace Mem0's internal extraction with our own, ensuring every memory in the system went through our quality filters.

**Changes:**

1. **Deprecate sidecar `/add` for runtime use** — The existing `/add` endpoint calls `mem.add()` which runs Mem0's extraction. Keep it for backward compatibility but all new runtime paths use `/add_direct` and `/add_batch`.

2. **`infrastructure/memory/sidecar/aletheia_memory/routes.py`** — Enhance `/add_direct` and `/add_batch`:
   - Content hash dedup (already exists in backfill, reuse)
   - Semantic dedup (cosine > 0.90 against top 5 existing results)
   - Metadata: `{ source, agent_id, user_id, created_at, session_id, confidence }`
   - Optional: entity extraction for Neo4j episode linking (reuse `_extract_entities_for_episode` but don't create new Entity nodes — only link to existing ones)

3. **`infrastructure/runtime/src/distillation/extract.ts`** — Quality improvements:
   - Add `## Bad Examples` section to prompt showing actual noise from our corpus:
     - "Uses grep" → skip (tool invocation, not knowledge)
     - "Familiar with confabulation guards" → skip (too generic)
     - "Works with a system called Aletheia" → skip (obvious from context)
   - Add `## Good Examples` with real high-value extractions:
     - "Prefers chrome-tanned leather for belts; rejects veg-tan for durability reasons"
     - "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions go to user_id=default"
     - "Pitman arm torque spec: 185 ft-lbs (not 225 as previously stated)"
   - Hard filters in code (not just prompt): skip items <15 chars, skip items matching noise patterns (`/^(Uses|Familiar with|Works with|Has experience)/i`)

4. **One-time Qdrant cleanup** — Purge remaining `source: after_turn` entries that came through Mem0's extraction (607 items). Replace with clean re-extraction from session history if valuable conversations exist.

**Acceptance Criteria:**
- [ ] All new memories bypass Mem0 extraction, use our prompt or direct storage
- [ ] Noise rate in new extractions: <5% (manual review of 50 random entries)
- [ ] Hard filters catch obvious garbage before it hits the vector store
- [ ] Legacy `/add` endpoint still works for external callers but runtime never uses it

**Testing:**
- Unit test: noise patterns are filtered
- Unit test: extraction prompt produces structured, specific facts from sample conversations
- Comparison test: same conversation through Mem0 extraction vs our extraction, count noise items

**LOC estimate:** ~100 (sidecar endpoint enhancement) + ~80 (extract.ts prompt + filters) + ~50 (cleanup script) = **~230**

---

### Phase 4: Entity Resolution and Graph Quality

**Scope:** Make Neo4j useful by solving entity resolution and enabling entity-to-entity relationships.

**Changes:**

1. **`infrastructure/memory/sidecar/aletheia_memory/entity_resolver.py`** (new) — Entity resolution service:
   - Maintain a canonical entity registry (stored in Neo4j or SQLite)
   - Before creating a new entity: fuzzy match against existing entities (Levenshtein + embedding similarity)
   - Merge rules: `aletheia_runtime` + `aletheia` + `aletheia_system` → canonical `aletheia`
   - Manual alias table for known equivalences
   - Expose `/entities/resolve` endpoint for batch resolution

2. **`infrastructure/memory/sidecar/aletheia_memory/routes.py`** — Upgrade entity extraction:
   - After extracting entities from text, resolve against canonical registry
   - Only create new Entity nodes for genuinely new entities (not fuzzy matches of existing ones)
   - Extract entity-to-entity relationships from facts: "1997 Ram 2500 steering box needs replacement" → `1997_ram_2500 -[HAS_PART]→ steering_box`, `steering_box -[NEEDS]→ replacement`
   - Use LLM (Haiku) for relationship extraction: input = fact + entity list, output = `[(entity1, rel_type, entity2)]`

3. **Entity schema** — Canonical entity nodes get:
   - `canonical_name`: primary label
   - `aliases`: list of known variants
   - `entity_type`: person | project | tool | vehicle | material | concept | location | organization
   - `first_seen`: when first extracted
   - `last_referenced`: when last mentioned in a conversation
   - `mention_count`: how many times referenced

**Acceptance Criteria:**
- [ ] Entity resolution prevents duplicate nodes (fuzzy match threshold: 0.85)
- [ ] Entity-to-entity relationships exist in the graph
- [ ] Graph-enhanced search returns meaningfully connected entities, not random names
- [ ] Entity types are consistent (no `person` label on `claude`)

**Testing:**
- Unit test: "aletheia_runtime" resolves to canonical "aletheia" node
- Unit test: fact with two entities produces entity-to-entity relationship
- Integration test: add 3 facts about the same project → single entity node with 3 relationships

**LOC estimate:** ~300 (entity resolver) + ~150 (extraction upgrade) + ~50 (schema) = **~500**

---

### Phase 5: Memory Lifecycle — Confidence, Contradiction, Correction

**Scope:** Memories should age, be correctable, and detect contradictions.

**Changes:**

1. **Confidence scoring** — Every memory gets a confidence score (0.0–1.0):
   - `1.0`: user explicitly stated ("I prefer chrome-tanned leather")
   - `0.8`: extracted from clear discussion context
   - `0.6`: inferred from behavior (used tool X repeatedly → "familiar with X")
   - Score decays: `-0.05` per 30 days unreferenced
   - Score reinforces: `+0.1` when referenced in a new conversation (capped at 1.0)
   - Recall weights `confidence * similarity_score` for ranking

2. **Contradiction detection** — During extraction:
   - Before storing a new fact, search for semantically similar existing facts (cosine > 0.80)
   - If found, compare: are they reinforcing (same fact restated) or contradicting?
   - Reinforcing: boost confidence of existing, skip new
   - Contradicting: flag both, store new with `contradicts: [old_id]` metadata
   - Surface contradictions during recall: "⚠️ Conflicting memories: [A] vs [B]"

3. **Correction tools** — Upgrade `fact_retract`:
   - `memory_correct(id, corrected_text)` — marks old as superseded, stores corrected version with `corrects: old_id`
   - `memory_forget(query)` — semantic search → soft-delete matching memories
   - Both exposed as agent tools and API endpoints

4. **Access tracking** — When recall surfaces a memory:
   - Record access timestamp and session context
   - Use access patterns for decay scoring
   - Surface "frequently accessed" memories with higher confidence

**Acceptance Criteria:**
- [ ] Memories have confidence scores that decay over time
- [ ] Contradicting facts are detected and flagged during extraction
- [ ] Agents can correct and retract memories via tools
- [ ] Recall rankings incorporate confidence alongside similarity

**Testing:**
- Unit test: confidence decay after 30 days
- Unit test: contradicting fact detected and flagged  
- Integration test: correct a memory → old version superseded, new version served
- Test: recall ranks high-confidence recent memory above low-confidence old memory

**LOC estimate:** ~200 (confidence model) + ~150 (contradiction detection) + ~100 (correction tools) + ~80 (access tracking) = **~530**

---

### Phase 6: Recall Quality — MMR, Thread Context, Domain Scoping

**Scope:** Make recall return better results by using conversation context, not just the last message.

**Changes:**

1. **MMR diversity re-ranking** (from Spec 07 Phase 3c):
   - After vector search returns top-K, apply Maximal Marginal Relevance
   - Penalize results that are semantically similar to already-selected results
   - Prevents "5 memories that all say the same thing" problem

2. **Thread-aware recall**:
   - Current: `recallMemories(messageText)` — searches only the current message
   - New: `recallMemories(messageText, threadSummary?)` — combines current message with thread summary for richer search context
   - Thread summary already exists in the thread model (`ThreadSummary` in finalize.ts)
   - Weight: 70% current message, 30% thread context

3. **Domain scoping** (from Spec 07 Phase 3a, partially done):
   - `domains` field already on NousConfig (PR #75)
   - Wire it: each agent's recall automatically scopes to their domains
   - Cross-domain search via explicit `mem0_search` tool call (no domain filter)
   - Prevents Demiurge from recalling pipeline architecture, Syn from recalling leather techniques

**Acceptance Criteria:**
- [ ] Recall returns diverse results (no 3+ results saying the same thing)
- [ ] Thread context improves recall relevance (A/B test: with vs without)
- [ ] Domain scoping prevents cross-domain noise in automatic recall
- [ ] Manual `mem0_search` still searches everything

**Testing:**
- Unit test: MMR re-ranking selects diverse results over redundant high-score ones
- Unit test: thread context changes recall results meaningfully
- Integration test: Demiurge recall returns leather memories, not pipeline memories

**LOC estimate:** ~100 (MMR) + ~80 (thread-aware recall) + ~60 (domain wiring) = **~240**

---

## Implementation Order

| Phase | Effort | Impact | Dependencies |
|-------|--------|--------|-------------|
| **1: Wire extraction pipeline** | ~270 LOC | 🔴 Critical — no memories being created | None |
| **2: After-turn extraction** | ~280 LOC | 🔴 High — fresh memories | Phase 1 |
| **3: Extraction quality** | ~230 LOC | 🟡 High — noise reduction | Phase 1 |
| **4: Entity resolution** | ~500 LOC | 🟡 Medium — graph usefulness | Phase 3 |
| **5: Memory lifecycle** | ~530 LOC | 🟡 Medium — long-term quality | Phase 1 |
| **6: Recall quality** | ~240 LOC | 🟢 Medium — retrieval improvement | Phase 1 |

**Phase 1 is blocking everything.** Without it, the system generates zero persistent memories from conversations. Phases 2-3 can be parallelized after Phase 1. Phase 4-6 are independent of each other.

**Total estimated LOC: ~2,050**

---

## Relationship to Existing Specs

- **Spec 07 (Knowledge Graph):** Phase 1a (vector-first search) ✅ done. Phases 2c (graph UI), 3b-3f absorbed here as Phases 4-6. Spec 07 should be updated to reference this spec for remaining work.
- **Spec 19 (Sleep-time Compute):** Reflection pipeline exists but can't flush to memory. Phase 1 here fixes that. Spec 19 is otherwise complete.
- **Spec 12 (Session Continuity):** Distillation pipeline works but drops extracted facts. Phase 1 here completes the circuit.

---

## Open Questions

1. **Should after-turn extraction (Phase 2) use the same model as working-state extraction?** Both are lightweight Haiku tasks running in finalize.ts. Could combine into a single LLM call that produces both working state AND memory facts.

2. **Neo4j: fix or replace?** Phase 4 assumes fixing Neo4j. Alternative: rip it out entirely and store entity relationships as metadata on Qdrant vectors. Simpler, fewer services, but loses traversal queries. Current graph traversal adds latency but rarely changes results.

3. **Cost budget for after-turn extraction?** At ~$0.0001/turn on Haiku, a heavy day (200 turns across all agents) costs $0.02. Negligible. But if we switch to Sonnet for quality, it's 10x.

4. **Backfill from session history?** Once Phase 1 is working, should we re-extract from the ~1,600 existing sessions to seed the memory with historical facts? Pro: rich corpus immediately. Con: $5-10 in API costs, could introduce noise.

---

## Success Criteria

- **Memory freshness:** Facts from a conversation appear in recall within 60s of turn completion
- **Noise rate:** <5% of memories are generic/useless (down from ~13%)
- **Recall relevance:** Top-3 recalled memories are relevant to the conversation 80% of the time (currently ~50%)
- **Contradiction detection:** Conflicting facts are flagged, not silently co-existing
- **Zero manual cleanup needed:** The pipeline maintains quality autonomously after Phase 3
