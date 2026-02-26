# Phase 3: Graph Extraction Overhaul - Context

**Gathered:** 2026-02-25
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace 81% generic RELATES_TO noise in Neo4j with typed, meaningful relationships. Integrate neo4j-graphrag SimpleKGPipeline with controlled vocabulary, replace deprecated neo4j-driver, enforce type validation on all writes, and backfill existing edges. Target: RELATES_TO below 30% of total relationships.

</domain>

<decisions>
## Implementation Decisions

### Relationship vocabulary design
- Unified vocabulary shared across all agents (not per-domain segments)
- Medium granularity: ~15-25 relationship types
- Derive empirically from existing graph data first, then refine based on agent domains and recall needs
- Claude's discretion on final type list — no manual approval gate required

### Existing RELATES_TO migration
- LLM backfill: reclassify all existing RELATES_TO edges through the new typed extraction pipeline
- One-time migration script (not background process) — clean cutover with before/after separation
- Edges that can't be confidently reclassified: delete (don't keep as RELATES_TO or quarantine)
- No cost constraints on backfill — process every edge regardless of token cost

### Unknown type enforcement
- Normalize unknown types to nearest vocabulary match (embedding similarity or keyword mapping)
- Enforce at extraction time, before Neo4j write — no invalid data enters the graph

### Vocabulary evolution
- Claude's discretion on evolution strategy with sensible norms built in (e.g., logging frequent normalizations, surfacing candidates)
- Vocabulary file lives outside the repo (gitignored) — deployment-specific, not checked into Aletheia source
- The vocabulary *system* (loading, enforcement, normalization) is part of Aletheia codebase; the vocabulary *data* is per-deployment config

### Claude's Discretion
- Final vocabulary type list (derived empirically + domain-informed)
- Vocabulary evolution strategy (fixed vs semi-automatic growth, with sensible defaults)
- Backfill batch sizing and implementation details
- Normalization algorithm for unknown types
- neo4j-graphrag integration architecture
- Driver upgrade mechanics (neo4j-driver → neo4j 6.1.0)

</decisions>

<specifics>
## Specific Ideas

- Vocabulary should be grounded in real graph data patterns, not theoretical design
- Per-deployment vocabulary means different Aletheia instances can have different relationship types tuned to their agents' domains

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 03-graph-extraction-overhaul*
*Context gathered: 2026-02-25*
