# Memory dedup: making auto-merge reachable

**Date:** 2026-05-28
**Author:** Stability v3
**Status:** Proposal (operator decision required before implementation)
**Tracks:** #4165 — `memory dedup` can never merge anything

## Question

`aletheia memory dedup` and the scheduled `entity-dedup` maintenance task are functionally inert on `main` — no entity merge can ever execute through any production code path. The CLI advertises "Find and merge duplicate entities/facts" and the maintenance task logs "N entities merged automatically", but `N` is structurally always 0. This proposal names two concrete paths to make auto-merge reachable, lays out the trade-offs, and recommends one. **No implementation begins without operator ratification of the choice.**

## Why it's unreachable

The merge score is a weighted composite (`crates/episteme/src/dedup.rs:123-135`, weights at `:100-106`):

```
score = 0.4·name_sim + 0.3·embed_sim + 0.2·type_match + 0.1·alias_overlap
```

`MergeDecision::from_score` (`dedup.rs:66-74`): AutoMerge needs `score ≥ 0.90`, Review is `0.70–0.90`, else Skip.

The two — and *only* — production callers of `generate_candidates` pass closures that **always return 0.0** for embedding similarity:

- `find_duplicate_entities` (display/preview): `crates/episteme/src/knowledge_store/entity.rs:141`
- `run_entity_dedup` (the executor): `entity.rs:355`

With `embed_sim ≡ 0.0` the maximum achievable score is:

```
0.4·1.0 + 0.3·0.0 + 0.2·1.0 + 0.1·1.0 = 0.70 < 0.90
```

`from_score` can therefore never return `AutoMerge` and the `execute_merge` loop is dead code. Review-tier candidates accumulate in `pending_merges` but `approve_merge` (the only function that would execute a queued merge) has **zero callers** — no CLI subcommand, no HTTP route, no test — so the review queue is unactionable too. Both halves of dedup are no-ops; the system reports success.

## Two concrete paths

### Path A — Wire real embedding similarity end-to-end

Replace both `|_,_| 0.0` closures with cosine-similarity over entity-name embeddings stored alongside each entity. This is the option the original `0.3·embed_sim` design weight presumes.

**Implementation surface:**

1. **Schema migration.** Today `load_entity_infos` selects `{id, name, entity_type, aliases, created_at}` (`entity.rs:401-408`). Add a nullable `name_embedding: Option<Vec<f32>>` column to the entities relation (Cozo).
2. **Lifecycle: when is the embedding generated?** Two sub-options:
   - **At entity-creation time.** Cheap when a provider is configured (most installs); falls back to `null` in degraded mode. Caller already has the name string and an `EmbeddingProvider` in scope.
   - **Lazily at dedup-scan time.** Avoids embedding churn for entities that never get dedup'd; requires provider availability at scan time, which is a deeper refactor.

   *Recommend at-creation.* The cost is paid once per entity ever, not on every dedup pass; the cache survives across runs; and the dedup scan doesn't need to acquire the provider.
3. **Plumb `EmbeddingProvider` to dedup callers.** Both `run_entity_dedup` and `knowledge_maintenance::run_entity_dedup_maintenance` currently have no provider in scope. With the at-creation lifecycle this becomes optional (only needed for entities that pre-date the migration); a degraded path computes `embed_sim = 0.0` and stays in current behavior for those, which is acceptable.
4. **Backfill.** A one-shot migration to embed names of pre-existing entities. Either eager (during the schema migration) or lazy (compute on first dedup pass that observes a null). Eager is simpler; lazy is friendlier to large installs.

**Cost estimate:** 1–2 weeks of focused work. Touches the Cozo schema, the `episteme::knowledge_store::entity` API, the maintenance task graph (provider plumbing), CLI bootstrap, and dedup tests.

**What gets fixed:** auto-merge becomes reachable for entities whose semantic-similarity score crosses 0.90 (a meaningful threshold — name + type + alias alone reach 0.70, and embedding similarity of 0.67 across the remaining 0.30 weight closes the gap). The review queue starts filling with semantically-meaningful candidates rather than near-exact name matches.

**What it doesn't fix:** degraded-mode installs (no embedding provider) stay in current behavior. That's correct — the design weight already accounts for this with the 70% non-embedding floor.

**Risks:**

- *Embedding-quality dependency.* If the configured embedding model is poor (small MiniLM, mismatched domain), `embed_sim` becomes noise and we trade a known-broken AutoMerge for a flaky one. Mitigated by keeping the 0.90 threshold (forces multi-signal agreement) and by the review tier (operator sees Review entries before AutoMerge would fire on borderline cases).
- *PII surface.* Entity names are arbitrary strings; storing embeddings of arbitrary strings in the database is no worse than storing the strings themselves, which we already do. No new privacy concern.
- *Migration risk.* A botched schema migration could corrupt existing entities. Mitigated by adding the column as nullable and not modifying existing rows — only writing on new entities and during the backfill scan.

### Path B — Re-weight to make name+type+alias reach AutoMerge

Lower the AutoMerge threshold to 0.70, or re-weight the score to give `name_sim`/`type_match`/`alias_overlap` enough mass to cross 0.90 on their own. Equivalent: disable AutoMerge entirely and treat the queue (Path A's mechanical bundle B) as the only execution path.

**Implementation surface:**

1. Constants change in `dedup.rs:66-74` (`MergeDecision::from_score`) and/or `dedup.rs:100-106` (weights).
2. Documentation update in `dedup.rs:110-118`.
3. Test update in `dedup_tests.rs` to assert the new threshold/weight semantics.

**Cost estimate:** 1 hour of code + indefinite empirical-validation overhead (operator must validate that the looser threshold doesn't produce bad merges on real data).

**What gets fixed:** auto-merge becomes reachable mechanically. The unreachability bug closes.

**What it breaks:** semantic safety. The 30% embedding weight existed precisely to *guard against* exact-name + same-type + alias-overlap producing a false positive. In collision-prone domains (two distinct people named "John Smith" both tagged `person` with alias `JS`) the looser threshold silently auto-merges them. The 0.30 embedding weight was the mechanism for catching that — a semantic check that "John Smith #1" and "John Smith #2" mention different contexts. Removing it without a semantic substitute is a quality regression on real-world data the moment any non-trivial entity catalog exists.

**The cheaper variant of B** is to leave the threshold and weights alone but **disable AutoMerge entirely** (clamp `from_score` to never return `AutoMerge`), then treat Path A's mechanical bundle B (an approval-path CLI for the review queue, separate proposal — see #4165's prior triage comment) as the only execution path. This is honest: it says "we don't have semantic similarity, so every merge is operator-reviewed." Cost: same hour. Semantics: no auto-merges happen, period; operators must drain the review queue manually.

## Recommendation

**Path A** if the cohort treats dedup as a real capability the system needs. The auto-merge weight design *presupposes* embedding similarity is provided; the bug is the implementation never wired it through, not that the design is wrong. Path A repairs the design.

**Path B-disabled** (the cheaper variant) if dedup is a low-priority feature and the cohort would rather block off AutoMerge as "operator-only" indefinitely. This is honest about the limitation and prevents the latent footgun of `--nous-id` tenant-bleed-on-merge (#4165 finding F) from going live the moment somebody re-enables AutoMerge without thinking.

**Path B (re-weight)** is *not* recommended in isolation. Lowering the threshold without restoring a semantic check makes the system less safe than today's bug. If the operator picks this, it should be paired with a strict opt-in flag (`--unsafe-name-only-merge`) so it's clear what trade-off was taken.

## What's separable from this decision (already named in #4165 triage)

Independent of Path A/B, six things ship safely under any contract and were bundled in the prior triage comment as "Phase 1 mechanical":

- **B (approval CLI)** — `aletheia memory dedup --approve <id>` / `--reject <id>` wired to existing `approve_merge`. Independently useful even if A.path stays unchanged.
- **D (config threshold plumbing)** — thread `taxis::config::AgentBehaviorDefaults::knowledge_dedup_*` through `generate_candidates` so tuning the config keys does something. Matches `dedup.rs:110-118`'s own doc-comments.
- **E (`--nous-id` scoping)** — replace `load_entity_infos(_nous_id)` with a real WHERE clause so the latent tenant-bleed footgun in F is eliminated *before* Path A makes it live.
- **F (test strengthening)** — add a sibling to `classify_auto_merge_and_review` that uses the production `embed=0.0` closure and asserts `auto_merge.is_empty()`. Catches A on day one of any future regression.
- **C-cheap** — drop "facts" from `--help` text (`mod.rs:27`); honest advertising.

These can ship as a single mechanical PR after operator picks A or B. They don't need contract ratification; they fix the system around the dedup core regardless of how the core is decided.

## Out of scope (escalate separately)

- **C-hard** — real fact-merge implementation (which fact wins? merge metadata? distinct nous-ids holding identical fact-content? 10k cap pagination?). This is a feature-design conversation and should get its own issue.
- **Cross-nous merge semantics** — if A.path lands and `load_entity_infos` becomes nous-scoped (E), the question of whether dedup ever merges *across* nous-ids arises. Today it's structurally impossible because dedup is broken; once it's fixed, the answer needs to be explicit.

## Decision needed from operator

Please ratify one of:

1. **Path A** — accept the 1–2 week schema-migration scope and the embedding-quality dependency.
2. **Path B-disabled** — accept that AutoMerge is permanently disabled until/unless a future design wires real semantics.
3. **Path B (re-weight)** — accept the false-positive risk on real-world data and pair with `--unsafe-name-only-merge` opt-in.

Implementation begins on the chosen path only after sign-off. The mechanical bundle (B/D/E/F + C-cheap) can ship independently once that decision is in hand.
