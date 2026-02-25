# Aletheia Memory System Audit & Overhaul

## What This Is

A comprehensive audit and overhaul of Aletheia's entire memory subsystem — spanning extraction, storage, recall, self-maintenance, and the graph layer. The goal is a production-grade showcase: benchmark-quality recall, observable self-healing, and comprehensive test coverage.

## Core Value

Agents remember everything important, surface nothing irrelevant, and maintain their own memory health without intervention.

## Requirements

### Validated

<!-- Existing capabilities confirmed in codebase -->

- ✓ Multi-tier memory (preserved tail, structured summaries, context editing, working state, agent notes) — existing
- ✓ Session classification (primary/background/ephemeral) with different lifecycles — existing
- ✓ Multi-signal distillation triggers (token threshold + message count + context size + staleness) — existing
- ✓ Workspace flush (daily markdown files from distillation) — existing
- ✓ After-turn fact extraction (async, Haiku) — existing
- ✓ Vector search via Qdrant with MMR diversity re-ranking — existing
- ✓ Neo4j graph storage (optional, degraded) — existing but broken
- ✓ Sleep-time compute (nightly reflection, weekly synthesis) — existing
- ✓ Domain-scoped memory per agent — existing
- ✓ Semantic dedup at cosine > 0.90 — existing
- ✓ Distillation receipts and logging — existing
- ✓ Memory confidence scoring with decay — existing

### Active

<!-- What this milestone delivers -->

- [ ] Full dead code audit and removal across memory modules
- [ ] Fix Neo4j to produce meaningful, typed relationships (not 81% generic RELATES_TO)
- [ ] Eliminate orphaned Qdrant entries (607+ source:after_turn from dead paths)
- [ ] Fix workspace flush (silently failing across distillation cycles)
- [ ] Evaluate Mem0 sidecar — keep, replace, or direct-to-Qdrant permanently
- [ ] End-to-end test coverage for all memory paths (extraction → storage → recall)
- [ ] Recall quality improvements — reduce noise, increase relevance
- [ ] Self-healing memory — automatic detection and repair of contradictions, stale entries, drift
- [ ] Fix distillation locking (in-memory Set, no crash recovery)
- [ ] Distillation cancellation support (AbortSignal threading)
- [ ] Session rollback on distillation failure (transactional safety)
- [ ] Observable memory health — metrics, diagnostics, audit tools
- [ ] Noise filtering hardening (beyond current regex patterns)

### Out of Scope

- Distributed multi-instance deployment — single-server is the target
- Replacing Qdrant or Neo4j with entirely different vector/graph engines
- UI changes beyond memory health visibility
- Changes to non-memory modules (auth, Signal, etc.) unless blocking memory work

## Context

The memory system spans 6+ modules: `mneme` (session store), `distillation` (compression pipeline), `nous` (recall, turn-facts, working state), `koina` (memory-client), the memory sidecar (Python FastAPI + Mem0 + Qdrant + Neo4j), and `daemon` (reflection cron jobs).

Known issues from spec archive:
- Mem0's extraction was bypassed in favor of direct Qdrant `/add_direct` writes
- 13% of Qdrant was noise in last audit, 81% of Neo4j relationships were generic
- Recall frequently hit 3s timeouts due to Neo4j + Qdrant compound latency
- Semantic drift causes cross-domain bleed (e.g., "tools" matching both leatherwork and vehicles)
- A session had 1,916 messages across 13 distillation cycles with zero workspace persistence
- Token estimation cache (`tokenCountEstimate`) can go stale between distillation and next message

Current pain points: shallow recall, hit-or-miss relevance, noise in surfaced memories, insufficient real-world testing of the full pipeline.

## Constraints

- **Stack**: TypeScript runtime, Python FastAPI sidecar, Qdrant, Neo4j — all self-hosted
- **Testing**: vitest, 80% coverage threshold, behavior-focused tests
- **Compatibility**: Must not break existing agent sessions or stored data
- **Neo4j**: Fix it properly, don't remove it — graph relationships have value if done right

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Fix Neo4j rather than remove | Graph relationships add value for relationship reasoning, concept clustering | — Pending |
| Evaluate Mem0 sidecar replacement | Direct-to-Qdrant writes already bypass most of Mem0; may be dead weight | — Pending |
| Production-grade showcase target | Memory is Aletheia's core differentiator; worth the investment | — Pending |

---
*Last updated: 2026-02-24 after initialization*
