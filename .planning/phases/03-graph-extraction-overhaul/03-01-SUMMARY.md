---
phase: 03-graph-extraction-overhaul
plan: 01
subsystem: database
tags: [neo4j, neo4j-graphrag, vocab, graph, python, fastapi]

# Dependency graph
requires:
  - phase: 02-data-integrity
    provides: metadata enforcement on /add_batch and session wiring
provides:
  - neo4j>=6.1.0 dependency for SimpleKGPipeline compatibility
  - neo4j-graphrag[anthropic]>=1.13.0 in pyproject.toml
  - load_vocab() reading external ~/.aletheia/graph_vocab.json with hardcoded fallback
  - normalize_type() returning None for unknown types (never RELATES_TO)
  - RELATES_TO removed from CONTROLLED_VOCAB, TYPE_MAP, and GRAPH_EXTRACTION_PROMPT
affects:
  - 03-graph-extraction-overhaul (plan 02 — SimpleKGPipeline integration uses new vocab and driver)

# Tech tracking
tech-stack:
  added:
    - neo4j>=6.1.0 (upgraded from 5.x)
    - neo4j-graphrag[anthropic]>=1.13.0 (new)
  patterns:
    - External vocab config at ~/.aletheia/graph_vocab.json with graceful fallback to hardcoded defaults
    - normalize_type() returns None for unknown types — callers skip rather than use catch-all
    - GRAPH_EXTRACTION_PROMPT instructs LLM to skip unmatched relationships (no fallback type)

key-files:
  created:
    - infrastructure/memory/sidecar/tests/test_driver_upgrade.py
    - infrastructure/memory/sidecar/tests/test_vocab.py
  modified:
    - infrastructure/memory/sidecar/pyproject.toml
    - infrastructure/memory/sidecar/aletheia_memory/vocab.py
    - infrastructure/memory/sidecar/aletheia_memory/config.py

key-decisions:
  - "normalize_type() returns None for unknown types instead of RELATES_TO — callers decide to skip rather than persist vague relationships"
  - "Vocab file at ~/.aletheia/graph_vocab.json uses version field and relationship_types list — fails safe to hardcoded defaults on missing/corrupt file"
  - "neo4j import test uses pytest.importorskip — neo4j not installed locally, pyproject.toml content check covers the spec constraint"
  - "GRAPH_EXTRACTION_PROMPT instructs LLM to skip relationships that don't match vocab, not fall back to a catch-all type"

patterns-established:
  - "External config loading pattern: read from ~/.aletheia/*.json, fall back to hardcoded defaults on any error"
  - "Strict vocab enforcement: normalize_type returns None, callers skip — no silent degradation to vague catch-all"

requirements-completed:
  - GRPH-04
  - GRPH-05
  - GRPH-06

# Metrics
duration: 3min
completed: 2026-02-25
---

# Phase 3 Plan 01: Driver Upgrade and Vocab Redesign Summary

**neo4j driver upgraded to 6.x, neo4j-graphrag added, RELATES_TO eliminated from vocab and prompt, external vocab loading from ~/.aletheia/graph_vocab.json**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-25T18:34:18Z
- **Completed:** 2026-02-25T18:37:39Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Upgraded neo4j dependency from 5.x to 6.1.0 and added neo4j-graphrag[anthropic]>=1.13.0
- Redesigned vocab.py: external config loading, normalize_type() returns None for unknowns, RELATES_TO removed everywhere
- Removed RELATES_TO from GRAPH_EXTRACTION_PROMPT and replaced "use as fallback" instruction with "skip if no match"

## Task Commits

1. **Task 1: Upgrade neo4j driver and add neo4j-graphrag dependency** - `4bb2452` (feat)
2. **Task 2: Redesign vocabulary system with external config and strict normalization** - `bf965ef` (feat)

**Plan metadata:** (docs commit — see below)

## Files Created/Modified

- `infrastructure/memory/sidecar/pyproject.toml` - neo4j bumped to >=6.1.0, neo4j-graphrag[anthropic]>=1.13.0 added
- `infrastructure/memory/sidecar/aletheia_memory/vocab.py` - Complete redesign: load_vocab(), external config, normalize_type returns None
- `infrastructure/memory/sidecar/aletheia_memory/config.py` - RELATES_TO removed from GRAPH_EXTRACTION_PROMPT
- `infrastructure/memory/sidecar/tests/test_driver_upgrade.py` - 5 tests for version constraints and import pattern
- `infrastructure/memory/sidecar/tests/test_vocab.py` - 21 tests for loading, normalization, and prompt

## Decisions Made

- `normalize_type()` returns `None` for unknown types instead of `"RELATES_TO"` — callers skip the relationship rather than persist a vague catch-all edge that degrades graph quality
- Vocab file uses JSON structure `{"version": 1, "relationship_types": [...], "fallback_type": null, "normalization_log": true}` — versioned for forward evolution
- `pytest.importorskip("neo4j")` used for the live import test — neo4j isn't installed in the local dev environment (server-side dep), pyproject.toml version string check covers the constraint
- `GRAPH_EXTRACTION_PROMPT` updated to say "skip it entirely" instead of "use RELATES_TO as fallback" — aligns with strict vocab policy for Plan 02's SimpleKGPipeline integration

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Test regex too broad — matched comment text, not import statement**
- **Found during:** Task 1 (test_driver_upgrade.py verification)
- **Issue:** `assert "neo4j_driver" not in content` matched "# Neo4j dri..." comment in graph.py — false positive
- **Fix:** Changed to `re.search(r"import neo4j_driver", content)` to check actual import statements only
- **Files modified:** infrastructure/memory/sidecar/tests/test_driver_upgrade.py
- **Verification:** test_graph_py_uses_neo4j_not_driver passes
- **Committed in:** `4bb2452` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - bug in test logic)
**Impact on plan:** Fix was necessary for correct test behavior. No scope creep.

## Issues Encountered

- neo4j package not installed in local dev environment — test_neo4j_graphdatabase_import uses `pytest.importorskip` to skip gracefully. The pyproject.toml version check (`test_pyproject_has_neo4j_6x`) covers the requirement without needing a live import.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- neo4j 6.x driver upgrade is a prerequisite for neo4j-graphrag SimpleKGPipeline — complete
- RELATES_TO fully eliminated from vocab, normalization, and LLM prompt — Plan 02 can enforce clean vocab
- normalize_type() now returns None for unknowns — Plan 02 callers need to handle None (skip relationship)
- External vocab file at ~/.aletheia/graph_vocab.json will be populated by Plan 02 or left to fall back to hardcoded defaults

---
*Phase: 03-graph-extraction-overhaul*
*Completed: 2026-02-25*
