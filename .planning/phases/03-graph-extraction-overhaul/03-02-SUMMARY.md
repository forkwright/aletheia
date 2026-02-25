---
phase: 03-graph-extraction-overhaul
plan: 02
subsystem: memory-sidecar
tags: [graph-extraction, neo4j-graphrag, SimpleKGPipeline, schema-enforcement, mem0]
dependency_graph:
  requires: ["03-01"]
  provides: ["03-03"]
  affects: ["infrastructure/memory/sidecar"]
tech_stack:
  added: ["neo4j-graphrag SimpleKGPipeline"]
  patterns: ["fire-and-forget graph extraction", "schema-enforced relationship types", "OAuth monkey-patch for graphrag LLM"]
key_files:
  created:
    - infrastructure/memory/sidecar/aletheia_memory/graph_extraction.py
    - infrastructure/memory/sidecar/tests/test_graph_extraction.py
  modified:
    - infrastructure/memory/sidecar/aletheia_memory/config.py
    - infrastructure/memory/sidecar/aletheia_memory/app.py
    - infrastructure/memory/sidecar/aletheia_memory/routes.py
key_decisions:
  - "SimpleKGPipeline cached at module level in graph_extraction._pipeline — reinit on OAuth rotation via refresh_pipeline_on_token_rotate"
  - "extract_graph / extract_graph_batch are fire-and-forget — failures log warning but never block memory writes"
  - "Removed CONTROLLED_VOCAB and normalize_type imports from routes.py — now only used by graph_extraction.py"
  - "Additional_relationship_types: False is the hard enforcement mechanism — vocab is enforced at pipeline write time, not post-write"
metrics:
  duration: "~6 min"
  completed: "2026-02-25"
  tasks_completed: 2
  tasks_total: 2
  files_modified: 5
---

# Phase 3 Plan 2: SimpleKGPipeline Integration Summary

SimpleKGPipeline wired as the graph extraction engine, replacing Mem0's graph store + post-write normalization with schema-enforced pre-write filtering via `additional_relationship_types: False`.

## Tasks Completed

| Task | Description | Commit |
|------|-------------|--------|
| 1 | Create graph_extraction.py with SimpleKGPipeline wrapper and LLM adapter | 5a60886 |
| 2 | Disable Mem0 graph store and wire routes to SimpleKGPipeline | 7723bb9 |

## What Was Built

### graph_extraction.py (new)

Core integration module. Key design points:

- `_SCHEMA` — schema dict with `additional_relationship_types: False` for hard enforcement. Uses `sorted(CONTROLLED_VOCAB)` as relationship_types.
- `create_graphrag_llm(backend)` — builds a `neo4j_graphrag.llm.AnthropicLLM` appropriate to the backend tier:
  - API-key: direct instantiation with key
  - OAuth: instantiate with placeholder, monkey-patch `anthropic_client` with OAuth-authenticated `anthropic.Anthropic(auth_token=token)`
  - Ollama / none: returns `None`
- `init_pipeline(backend)` — combines LLM + neo4j driver + schema into a `SimpleKGPipeline`. Returns `None` if Neo4j unavailable or no compatible LLM.
- `extract_graph(text, backend)` — async, catches all errors, returns `{"ok": True/False}`. Safe for fire-and-forget.
- `extract_graph_batch(texts, backend)` — joins texts with `\n\n` before calling `extract_graph`, avoiding N LLM calls for batches.

### config.py

Removed `graph_store` key and `custom_prompt` key from `build_mem0_config()`. Mem0 now does vector-only writes. The `custom_fact_extraction_prompt` (FACT_EXTRACTION_PROMPT) for Mem0's fact extraction is unchanged.

### app.py

- Removed `mem.graph` patching block from `_inject_oauth_llm()` — the graph attribute no longer exists without a Mem0 graph store.
- Added `init_pipeline(_active_backend)` call in `lifespan()` after `Memory.from_config(config)`. Result stored in `app.state.graph_pipeline`.
- Added `refresh_pipeline_on_token_rotate(app)` function for callers to use when the OAuth token rotates.

### routes.py

- Added `from .graph_extraction import extract_graph, extract_graph_batch` import.
- `/add`: replaced `asyncio.create_task(_normalize_neo4j_relationships())` with `asyncio.create_task(extract_graph(req.text, backend=backend))`.
- `/add_batch`: added `asyncio.create_task(extract_graph_batch(new_texts, backend=backend))` after successful Qdrant upsert.
- `/add_direct`: added `asyncio.create_task(extract_graph(text, backend=backend))` after Qdrant upsert.
- Removed `_normalize_neo4j_relationships()` function entirely.
- Removed `/normalize_relationships` POST endpoint.
- Removed now-unused `CONTROLLED_VOCAB` and `normalize_type` imports from vocab.

## Test Results

49 passed, 1 skipped (neo4j driver skip — server-side dep). 16 new tests in `test_graph_extraction.py`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] pytest-asyncio not installed, async tests needed `asyncio.run()` wrapper**
- **Found during:** Task 1, first test run
- **Issue:** `@pytest.mark.asyncio` requires `pytest-asyncio` package, which is not installed. The other test files in the project use synchronous tests only.
- **Fix:** Replaced `@pytest.mark.asyncio async def test_X()` pattern with `def test_X(): asyncio.run(async_body())` pattern, consistent with the project's test approach.
- **Files modified:** `tests/test_graph_extraction.py`
- **Commit:** 5a60886 (included in initial task 1 commit)

## Self-Check: PASSED

- FOUND: infrastructure/memory/sidecar/aletheia_memory/graph_extraction.py
- FOUND: infrastructure/memory/sidecar/tests/test_graph_extraction.py
- FOUND: commit 5a60886 (Task 1)
- FOUND: commit 7723bb9 (Task 2)
