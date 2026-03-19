# R1466: Causal Reasoning in Knowledge Graph

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1466

---

## Executive Summary

The `mneme` knowledge graph stores facts, entities, and typed relationships, but all edges are associative — there is no representation of causation, temporal precedence, or counterfactual dependency. Adding causal edges enables the agent to reason about *why* things happened (not just *that* they did), propagate confidence through causal chains, and surface root-cause explanations rather than symptom descriptions.

**Recommendation: Implement incrementally.** Phase 1 adds causal edge types and a simple path query to the existing CozoDB schema. Phase 2 (future) adds probabilistic propagation and counterfactual generation. The Phase 1 scope is small and does not require changes outside `mneme`.

---

## 1. Problem Statement

The current knowledge graph supports these relationship types (inferred from `mneme`'s schema): `related_to`, `contradicts`, `supersedes`, `supports`, `mentions`, and entity relationships like `works_at`, `is_a`, `part_of`. All are **symmetric or directional-but-associative** — they express co-occurrence or logical proximity, not causation.

This creates gaps:

- The agent can recall "deployment failed" and "config changed" as related facts, but cannot reason that the config change *caused* the deployment failure.
- Confidence in derived facts cannot propagate from cause to effect — a retracted root cause should reduce confidence in downstream effects.
- Post-incident analysis and root-cause surfacing require traversing causal chains, which are not queryable today.

---

## 2. Proposed Approach

### 2.1 Causal Edge Types

Extend the relationship schema in `mneme` with three new edge kinds:

| Edge type | Meaning | Directionality |
|---|---|---|
| `causes` | A directly produces B (mechanism known) | A → B |
| `contributes_to` | A is a partial cause of B (probabilistic) | A → B |
| `prevented_by` | A would cause B, but C intervenes | A → B, C ↗ |
| `temporally_precedes` | A happened before B; causal link uncertain | A → B |

The `causes` edge is the primary type. `contributes_to` is for probabilistic or multi-factor causation. `temporally_precedes` is the weakest claim — it records sequence without asserting mechanism.

### 2.2 Schema Extension (CozoDB Datalog)

```datalog
# New relation: causal edges
:create causal_edge {
    from_fact: Uuid,
    to_fact: Uuid,
    edge_kind: String,        # "causes" | "contributes_to" | "prevented_by" | "temporally_precedes"
    confidence: Float,        # 0.0–1.0, operator-assigned or LLM-extracted
    mechanism: String?,       # free-text explanation of the causal pathway
    extracted_at: Timestamp,
    nous_id: Uuid,
}

# Index for forward traversal
:index causal_edge:from_fact

# Index for backward traversal (finding causes of an effect)
:index causal_edge:to_fact
```

The existing `relationship` table is retained for non-causal edges. Causal edges are a separate table to allow distinct confidence semantics and query patterns.

### 2.3 Confidence Propagation

When a causal chain `A causes B causes C` exists:

```
confidence(C via chain) = confidence(A) × strength(A→B) × strength(B→C)
```

This is analogous to belief propagation in a Bayesian network, but simplified: each `causes` edge carries a `confidence` (default 1.0 for direct causation, lower for `contributes_to`).

Propagation rules (Datalog):

```datalog
# Direct causal confidence
causal_confidence[fact, confidence] <-
    causal_edge[from: root, to: fact, confidence: c],
    fact_confidence[root, rc],
    confidence = rc * c

# Transitive (depth-limited to 5 hops to avoid cycles)
causal_confidence[fact, confidence] <-
    causal_edge[from: intermediate, to: fact, confidence: c],
    causal_confidence[intermediate, ic],
    confidence = ic * c
```

Cycle detection: add a `visited` accumulator in the Datalog query and break when `from_fact` appears in the chain.

### 2.4 Extraction Pipeline Integration

Extend the LLM-driven fact extraction in `mneme` to also extract causal relationships:

**Prompt addition:**
```
After extracting facts and entities, identify any causal relationships between
facts. For each causal pair, specify:
- which fact causes which
- the edge type (causes / contributes_to / temporally_precedes)
- a brief mechanism description
- your confidence (0.0–1.0)

Output causal edges only when the causal direction is clear from the text.
Do not infer causation from mere co-occurrence.
```

The extraction result schema gains:

```rust
pub struct ExtractedCausalEdge {
    pub from_description: String,   // matched to fact by embedding similarity
    pub to_description: String,
    pub edge_kind: CausalEdgeKind,
    pub mechanism: Option<String>,
    pub confidence: f32,
}
```

Matching `from_description` / `to_description` to existing facts uses the same embedding lookup as entity deduplication.

### 2.5 Query API

New methods on `KnowledgeStore`:

```rust
// Find all facts that caused the given fact
fn find_causes(&self, fact_id: FactId, max_depth: u8) -> Vec<CausalChain>

// Find all downstream effects of a fact
fn find_effects(&self, fact_id: FactId, max_depth: u8) -> Vec<CausalChain>

// Root cause analysis: trace back to facts with no incoming causal edges
fn root_causes(&self, fact_id: FactId) -> Vec<FactId>

// Confidence-weighted causal path score
fn causal_path_confidence(&self, from: FactId, to: FactId) -> Option<f32>
```

### 2.6 Recall Integration

Extend the 6-factor recall scorer in `mneme` with a **causal relevance** signal:

- If the query contains causal language ("why", "caused by", "because of", "led to"), boost facts that are endpoints of causal edges.
- Specifically: facts that are roots of causal chains (have outgoing `causes` edges but no incoming) score higher for "why" queries.

This requires detecting causal query intent — a simple keyword heuristic is sufficient for Phase 1.

### 2.7 Built-in Tool: `memory_audit` Extension

The existing `memory_audit` tool surfaces facts for review. Extend it to also show:

- Causal chains with low-confidence links (flagged for human review)
- Conflicting causal explanations (two facts claim to cause the same effect)
- Orphaned causal edges (where one endpoint fact has been retracted)

---

## 3. Alternatives Considered

### 3.1 Use Standard Relationship Edges with a `causal` Flag

Add `is_causal: bool` to the existing `relationship` table.

**Rejected.** Loses the directionality semantics and confidence propagation model. The query pattern for causal traversal is different enough to warrant a separate table.

### 3.2 Full Probabilistic Graphical Model (Bayesian Network)

Implement belief propagation over a full DAG with prior/likelihood structure.

**Deferred.** Correct but requires a dedicated inference engine. The simplified multiplicative confidence propagation in Phase 1 captures 80% of the value with 10% of the complexity.

### 3.3 Pearl's Do-Calculus for Counterfactuals

Add counterfactual reasoning ("what would have happened if A had not occurred?").

**Deferred to Phase 3.** Requires interventional distribution modeling on top of causal graph. Out of scope for the initial implementation.

### 3.4 External Graph DB (Neo4j, etc.)

Move the knowledge graph to a dedicated graph database with native causal traversal.

**Rejected.** Increases operational complexity significantly. CozoDB's Datalog is sufficient for the depth-5 traversal patterns needed here.

---

## 4. Open Questions

1. **Cycle handling:** Causal graphs must be DAGs, but extracted causal edges from LLM output may introduce cycles (feedback loops). How should the extractor handle "A causes B causes A" patterns? Flag as `contributes_to` bidirectional?

2. **Cross-nous causal edges:** Can a fact in agent A's knowledge cause a fact in agent B's? The current schema partitions by `nous_id`. Cross-agent causal edges would require a shared causal layer.

3. **Retraction propagation:** When a cause fact is retracted (`memory_retract`), should all downstream effect facts be automatically flagged for review? Or silently reduced in confidence?

4. **Temporal vs causal:** Many facts are temporally ordered but not causally related. The `temporally_precedes` edge type risks polluting the graph with spurious causal candidates. Should temporal ordering be tracked separately from the causal table?

5. **LLM accuracy:** How reliably do current Claude models extract causal vs correlational relationships? Benchmark needed on aletheia's actual conversation corpus before relying on LLM-extracted causal edges.

6. **UI/tool exposure:** Should causal chains be surfaced in the `datalog_query` built-in tool? Or only via the new `find_causes`/`find_effects` API?

---

## 5. Implementation Sketch

```
crates/mneme/src/
  knowledge_store/
    causal.rs         # CausalEdge struct, insert/query operations
    schema.rs         # Add causal_edge table creation/migration
  recall.rs           # causal_relevance scorer for "why" queries
  extraction/
    causal.rs         # ExtractedCausalEdge, LLM prompt extension

crates/organon/src/builtins/
  memory_audit.rs     # extend to show causal chain issues
```

Migration: `causal_edge` table is additive; no changes to existing fact/relationship tables.

---

## 6. References

- Pearl, J. (2000). *Causality: Models, Reasoning, and Inference.* Cambridge University Press.
- CozoDB Datalog documentation (recursion and fixed-point semantics)
- `crates/mneme/src/knowledge_store/` — existing relationship schema
- `crates/mneme/src/recall.rs` — 6-factor recall scorer
- `crates/mneme/src/extraction/` — LLM-driven extraction pipeline
