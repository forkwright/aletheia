# Memory Architecture Recommendation
*Deep analysis based on peer feedback and 2025/2026 research*

## Peer Feedback Summary

### Chiron (Work)
1. **Structured retrieval** - SQL patterns need metadata (schema, table, metric), not just embeddings
2. **Temporal awareness** - "What changed since X?"
3. Session continuity secondary

### Eiron (School)  
1. **Temporal awareness** - #1 priority. Deadlines cascade, bandwidth matters
2. **Knowledge linking** - Cross-course connections rebuilt every session
3. "A calendar that *thinks*"

### Syl (Home)
1. **Temporal awareness** - Routines, "when last", evolving patterns
2. **Relationship mapping** - Who does what, family dynamics
3. "Current memory is too static"

**Common thread: TEMPORAL AWARENESS is #1 across all domains**

---

## Options Analysis

### Option A: Graphiti (Zep's Temporal Knowledge Graph)

**What it is:** Production-grade temporal knowledge graph that powers Zep's SOTA agent memory.

**Architecture:**
```
Episodes (raw messages) 
    ↓
Entities (extracted, resolved)
    ↓  
Communities (clusters, summaries)
```

**Key features:**
- **Bi-temporal model** - When it happened vs when we learned it
- Real-time incremental updates (no batch recomputation)
- Entity resolution across episodes
- Hybrid retrieval (semantic + keyword + graph traversal)
- **Has MCP server ready**

**Requirements:**
- Neo4j or FalkorDB (graph database)
- OpenAI API (for LLM inference)
- Python 3.10+

**Pros:**
- Designed specifically for temporal reasoning
- Production-proven (powers Zep, SOTA benchmarks)
- Handles contradictions via temporal edge invalidation
- Sub-second query latency
- Custom entity types supported

**Cons:**
- Requires graph database infrastructure
- OpenAI dependency (cost)
- Complexity overhead

**Fit score:** 9/10 (addresses all peer needs)

---

### Option B: A-Mem (Zettelkasten-style Agentic Memory)

**What it is:** NeurIPS 2025 paper - dynamic memory organization inspired by Zettelkasten note-taking.

**How it works:**
1. Each memory → comprehensive note (keywords, context, tags, embeddings)
2. LLM analyzes and generates metadata automatically
3. System finds relationships, establishes links
4. **Memory evolution** - new memories update old ones

**Requirements:**
- ChromaDB (vector store)
- LLM backend (OpenAI, Ollama, local)

**Pros:**
- Zettelkasten philosophy aligns with Cody's approach
- Simpler infrastructure (just ChromaDB)
- Multiple LLM backends (can use local)
- Memory evolution is elegant
- Production-ready code available

**Cons:**
- Less temporal focus than Graphiti
- Newer, less battle-tested
- Linking is semantic, not explicitly temporal

**Fit score:** 7/10 (good for linking, weaker on temporal)

---

### Option C: Enhance Existing System

**What we have:**
- Daily files (memory/YYYY-MM-DD.md)
- MEMORY.md (curated)
- facts.jsonl (atomic facts with confidence)
- Letta (queryable)

**Enhancement path:**
1. Add timestamps to facts (valid_from, valid_to)
2. Add structured metadata fields
3. Implement decay/reinforcement
4. Build temporal query layer

**Pros:**
- No new infrastructure
- Incremental improvement
- Preserves existing workflows

**Cons:**
- Won't achieve sophisticated temporal reasoning
- Manual implementation work
- May hit ceiling on complex queries

**Fit score:** 5/10 (addresses basic needs, limited ceiling)

---

### Option D: Hybrid Architecture (Recommended)

**Insight:** Don't replace what works. Layer temporal intelligence on top.

**Architecture:**
```
┌─────────────────────────────────────────────────┐
│  GRAPHITI (Temporal Knowledge Graph)            │
│  - Episodes from all interactions               │
│  - Entity extraction + resolution               │
│  - Temporal relationships                       │
│  - Community detection                          │
└─────────────────────────────────────────────────┘
         ↑                    ↑
         │                    │
┌────────┴────────┐  ┌────────┴────────┐
│ Domain Files    │  │ Google Calendar │
│ (as-is)         │  │ MCP             │
│ - SQL patterns  │  │ - Scheduling    │
│ - MBA notes     │  │ - Deadlines     │
│ - Home routines │  │ - Events        │
└─────────────────┘  └─────────────────┘
```

**Why hybrid:**
1. Graphiti provides temporal backbone (what all agents need)
2. Domain files stay domain-specific (what Chiron needs for SQL schemas)
3. Calendar MCP provides immediate scheduling awareness
4. No rip-and-replace, incremental adoption

---

## Recommended Implementation Plan

### Phase 1: Immediate (Tonight)
**Deploy Google Calendar MCP**
- Creds already exist at `~/.config/google-calendar/`
- Instant temporal awareness for scheduling
- Low effort, high impact

### Phase 2: This Week
**Add bi-temporal metadata to facts.jsonl**
```json
{
  "subject": "cody",
  "predicate": "prefers",
  "object": "semantic routing",
  "occurred_at": "2026-01-29T22:00:00",  // When it happened
  "learned_at": "2026-01-29T22:10:00",   // When we recorded it
  "valid_until": null,                    // Still true
  "confidence": 0.9
}
```
- Enables "what did we know at time X?" queries
- Foundation for decay/reinforcement

### Phase 3: Next Sprint (1-2 weeks)
**Integrate Graphiti for episode tracking**
- Use FalkorDB (simpler than Neo4j, Docker-ready)
- Ingest daily interactions as episodes
- Let it extract entities and relationships
- Build query layer for "when did X change?" / "what connects A to B?"

### Phase 4: Later (Month 2)
**A-Mem evolution patterns**
- Implement memory evolution (new updates old)
- Add automatic metadata generation
- Let communities emerge from usage patterns

---

## Decision Matrix

| Criterion | Weight | Graphiti | A-Mem | Enhance | Hybrid |
|-----------|--------|----------|-------|---------|--------|
| Temporal awareness | 40% | 10 | 6 | 4 | 9 |
| Structured retrieval | 20% | 8 | 7 | 6 | 8 |
| Knowledge linking | 20% | 9 | 9 | 5 | 8 |
| Implementation effort | 10% | 5 | 7 | 9 | 6 |
| Preserves existing | 10% | 6 | 6 | 10 | 9 |
| **Weighted Score** | | **8.3** | **6.9** | **5.5** | **8.3** |

**Tie-breaker:** Hybrid preserves existing system (risk mitigation) while getting Graphiti's temporal power.

---

## Final Recommendation

**Go with Hybrid (Option D):**

1. **Tonight:** Deploy Calendar MCP (30 min)
2. **This week:** Add temporal fields to facts.jsonl (2 hours)
3. **Next sprint:** Graphiti + FalkorDB integration (1-2 days)
4. **Later:** A-Mem evolution patterns (ongoing)

**Why this wins:**
- Addresses the #1 need (temporal) without disrupting existing system
- Incremental risk - can stop at any phase
- Calendar MCP provides immediate value
- Graphiti is proven at scale (Zep benchmarks)
- Preserves domain-specific patterns (Chiron's SQL schemas, etc.)

---

## Quick Start Commands

```bash
# Phase 1: Calendar MCP
npm install -g @cocal/google-calendar-mcp
# Already have creds at ~/.config/google-calendar/

# Phase 3: Graphiti
docker run -p 6379:6379 -p 3000:3000 -it --rm falkordb/falkordb:latest
pip install graphiti-core[falkordb]
```
