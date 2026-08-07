# Recall golden-set harness

A repeatable way to measure episteme's recall pipeline against a real
`shared`-cohort store, with human-in-the-loop judging (Recall@K, MRR) and a
drafted, unlabelled query set covering six retrieval-difficulty classes.

## Components

| Piece | Path | What it does |
|-------|------|---------------|
| Retrieval harness | `crates/aletheia/src/bin/golden_set_harness.rs` | Copy-verifies the `shared` cohort, runs every golden query through `KnowledgeStore::search_hybrid_scoped`, hydrates content, writes a JSON judging bundle. |
| Query set | `instance.example/data/eval/recall-golden-set.jsonl` | 91 drafted, unlabelled queries across 6 classes (see below). |
| Judging + scoring | `scripts/golden-set-judge.py` | `judge`: walk the bundle, collect human relevance labels efficiently. `score`: compute Recall@K / MRR, overall and per class, with an optional CI gate. |

## Two hard constraints

The harness is built so these are structural, not documentation:

1. **Psyche is excluded by construction.** There is no `--cohort` argument
   anywhere in the harness's CLI — the `shared` cohort name is a hardcoded
   constant, so no invocation can point it at `psyche`. Independently, the
   copy step refuses any path component literally named `psyche` found
   *below* the copy root (defense against a legacy drag-along nested
   `psyche` directory). Both refusals are covered by tests in
   `golden_set_harness.rs::tests`, not just documented.

   Psyche may still be **copied on-box** like any cohort (that's a
   different, already-solved problem — see
   `crates/episteme/src/knowledge_store/snapshot.rs` on
   `wave1/migration-atomicity`). What must never happen is psyche content
   reaching this tool's output, because this tool's whole purpose is
   handing retrieved text to a judging surface (a human today; potentially
   an automated evaluator later) — i.e. potentially off-box. Since this
   tool never resolves any cohort but `shared`, that question doesn't arise.

2. **The live keyspace is never opened.** The harness copies `shared` into
   a scratch directory, verifies the copy is genuinely restorable (fjall
   version marker present, full-scan record count matches a pre-copy count
   taken from the source) via a zero-background-worker open, and only ever
   queries the verified copy. Ported from `snapshot.rs`'s
   `pre_migration_snapshot` / `verify_restorable` (aletheia#5779) — write-new,
   verify, then replace; a failed copy or failed verify leaves the prior
   verified snapshot untouched.

## Running it

```sh
cargo run -p aletheia --bin golden_set_harness --features recall -- \
  --work-dir /tmp/golden-set-work \
  --queries instance.example/data/eval/recall-golden-set.jsonl \
  --out /tmp/golden-set-bundle.json \
  --top-k 10
```

`--instance-root` defaults to `$ALETHEIA_ROOT` or `./instance` (same
resolution as every other `aletheia` command, via `taxis::oikos::Oikos`).
The embedding provider and model are derived from the instance's own
`config/aletheia.toml` / `aletheia.json` (`embedding.*`) — not from a CLI
flag — so the query embeddings are guaranteed to match whatever the store
was actually built with. If config load fails, the harness hard-errors
rather than silently falling back to a default embedding model that would
corrupt the measurement.

Then judge:

```sh
scripts/golden-set-judge.py judge \
  --bundle /tmp/golden-set-bundle.json \
  --labels /tmp/golden-set-labels.jsonl
```

One line of input per query (a comma-separated list of relevant ranks, or
empty/`0` for "nothing relevant retrieved", or `s` to skip an ambiguous
query for a second pass) — not one prompt per retrieved item. Progress
saves after every query, so `judge` is safely interruptible and resumable;
re-running it only prompts for queries without a label yet (`--redo` to
re-judge, `--class <name>` to focus one class at a time).

Then score:

```sh
scripts/golden-set-judge.py score \
  --bundle /tmp/golden-set-bundle.json \
  --labels /tmp/golden-set-labels.jsonl \
  --k 5,10 --out /tmp/golden-set-report.json
```

## The regression-gate shape

`score --gate --min-coverage 0.9 --min-recall-at-k 10=0.7 --min-mrr 0.4`
exits non-zero when a floor is missed, in the same absolute-floor style as
`episteme::embedding_eval::EvalGateThresholds`. It does **not** yet support
a delta-vs-baseline check (`min_recall_at_k_delta` in the embedding-eval
gate) — that needs a persisted historical scored report to diff against,
and none exists yet. Once a few `score --out` reports have accumulated,
adding baseline diffing to `apply_gate()` in `golden-set-judge.py` is the
natural next step; until then CI (if wired) should pin explicit absolute
floors.

## Query classes

Drafted from the schema (`crates/eidos/src/knowledge/{fact,entity,causal,scope}.rs`)
and the recall code (`crates/episteme/src/recall/mod.rs`'s 11-factor scorer,
`crates/episteme/src/knowledge_store/search.rs`'s `HybridQuery`/tiered
search, `crates/episteme/src/recall/reranker.rs`'s `Reranker` trait) — never
from record content, which this session did not and must not read.

| Class | n | What it stresses |
|-------|---|-------------------|
| `entity_lookup` | 14 | Single-hop `Entity`/`Relationship` attribute lookups; a difficulty floor for the other classes. |
| `symptom_to_procedure` | 15 | Symptom fact → fix fact, plausibly linked by a `CausalEdge(Enabled)`; troubleshooting-shaped queries. |
| `temporal_bitemporal` | 14 | `valid_from`/`valid_to`/`recorded_at` distinctions, the far-future sentinel, supersession chains, "as of" queries. |
| `negation` | 14 | High lexical overlap with a false-positive polarity — the class BM25/embedding retrieval structurally struggles with. |
| `multi_hop` | 14 | Requires traversing ≥2 relationships/causal edges. **See limitation below: the harness runs with an empty `seed_entities` set by default, so this class currently only exercises BM25 + vector, not the graph signal it's meant to probe.** |
| `paraphrase` | 20 (10 pairs) | Same information need, two phrasings with low lexical overlap — tests recall consistency under rewording, independent of an LLM query-rewriter. |

## What this harness cannot measure yet

- **Tier-2 / graph-enhanced search.** `KnowledgeStore::search_tiered_for_recall_scoped`
  (query rewriting + graph expansion when the fast path is insufficient) is
  not exercised — only the deterministic fast path (`search_hybrid_scoped`).
  Wiring a `RewriteProvider` would need a live LLM call per escalated query,
  which trades determinism/repeatability for tier-2 coverage; a future
  iteration could add it as an opt-in mode.
- **Multi-hop graph seeding.** `HybridQuery.seed_entities` is empty for
  every query unless `--seed-entities-file <path>` supplies a
  `{query_id: [EntityId, ...]}` map. There is no entity-resolution step
  (query text → `EntityId`) in the harness, because building one would
  require reading real entity content to know what IDs exist — out of
  scope for a tool whose entire premise is not reading record content.
  `multi_hop`-class results should be read as "current BM25+vector recall
  for a graph-shaped question", not "graph-traversal recall".
- **Cross-nous private-visibility recall.** The default
  `--requester-nous-id golden-set-harness` is a synthetic identity that
  owns no facts, so `search_hybrid_scoped`'s visibility scoping only
  retains `Shared`/`Published` facts. Pass a real nous ID to additionally
  measure that nous's own `Private`-visibility recall; per-nous coverage
  across the whole fleet is not automated.
- **Ground truth.** The query set ships unlabelled by design (this session
  could not read record content). Recall@K/MRR are only as good as the
  accumulated human labels in `--labels`; `score`'s `coverage` field makes
  partial-judging progress visible rather than silently diluting the
  metric.
- **Baseline diffing in the gate.** See "The regression-gate shape" above.
