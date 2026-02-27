# v1.0 Memory Recall Baseline

**Version:** v1.0
**Date:** 2026-02-25
**Environment:** Production corpus runner (local execution, real agent session data)
**Corpus:** `infrastructure/runtime/tests/corpus/` — 22 sessions, real agent memory files

## Corpus Details

- **File:** `infrastructure/runtime/tests/corpus/baseline.json`
- **Generated:** 2026-02-25T14:55:50Z
- **Sessions:** 22 corpus entries
- **Agents covered:** akron, arbor, demiurge, eiron, syl, syn
- **Matching algorithm:** Jaccard token overlap, threshold = 0.3

## Overall Scores

| Metric    | Score  |
| --------- | ------ |
| Precision | 48.8%  |
| Recall    | 59.1%  |
| F1        | 53.4%  |

Raw: precision=0.4877, recall=0.5908, f1=0.5343
Counts: 218 matched / 447 extracted / 369 expected

## Per-Type Breakdown

| Type           | Precision | Recall | F1    | Matched | Extracted | Expected |
| -------------- | --------- | ------ | ----- | ------- | --------- | -------- |
| facts          | 57.6%     | 55.8%  | 56.7% | 121     | 210       | 217      |
| decisions      | 24.6%     | 40.5%  | 30.6% | 15      | 61        | 37       |
| contradictions | 50.0%     | 8.3%   | 14.3% | 1       | 2         | 12       |
| entities       | 46.6%     | 78.6%  | 58.5% | 81      | 174       | 103      |

## Per-Agent Breakdown

| Agent    | Precision | Recall | F1    | Matched | Extracted | Expected |
| -------- | --------- | ------ | ----- | ------- | --------- | -------- |
| akron    | 46.2%     | 55.0%  | 50.2% | 55      | 119       | 100      |
| arbor    | 65.0%     | 68.4%  | 66.7% | 13      | 20        | 19       |
| demiurge | 51.2%     | 73.3%  | 60.3% | 22      | 43        | 30       |
| eiron    | 40.5%     | 54.8%  | 46.6% | 17      | 42        | 31       |
| syl      | 58.2%     | 50.0%  | 53.8% | 32      | 55        | 64       |
| syn      | 47.0%     | 63.2%  | 53.9% | 79      | 168       | 125      |

## RELATES_TO Backfill Results

**Status:** No action required — 0 RELATES_TO edges found in production Neo4j.

**Formal confirmation:** Backfill script executed as syn user on 2026-02-27 with correct Neo4j credentials.

```
BEFORE: 0 RELATES_TO / 1194 total (0.0% rate)
  Detected 0 RELATES_TO edges to process
No RELATES_TO edges found. Nothing to do.
```

Phase 3 vocabulary enforcement (normalize_type() returning None for unknown types, `additional_relationship_types: False`) prevented all RELATES_TO edges from being written to Neo4j. No historical edges to reclassify.

## Notes

- These scores are the v1.0 benchmark for future regression detection
- Contradictions recall (8.3%) is the weakest signal — only 1/12 expected contradictions matched; extraction pipeline surface area is limited
- Decisions precision (24.6%) is low — over-extraction relative to ground truth; tunable via extraction prompt specificity
- Entity recall (78.6%) is the strongest signal — pipeline captures most entities but some false positives (precision 46.6%)
- Run `cd infrastructure/runtime && npm run test:corpus` to re-score against current pipeline
- Run `npm run test:corpus:save-baseline` to update this baseline after intentional extraction improvements
