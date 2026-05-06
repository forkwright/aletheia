# R722: Identity-continuity recall+extraction profile + private-nous fence primitives

## Question

Aletheia's recall and extraction pipelines are tuned for general task agents whose self-presentation drifts gracefully and whose memory is shared across the fleet by default. For nouses where consistent self-presentation across long sessions matters more than encyclopedic factual accuracy — and where the workspace is operator-private rather than shared infrastructure — we need a coordinated set of primitives that compose cleanly with the existing stack. What mechanics are required, where do they land in the codebase, and how do they relate to R716 (cross-agent knowledge sharing)?

## Findings

### Background — empirical motivation

The architectural intuition that this work formalizes has empirical backing:

- **Knowledge vs reasoning are separable in network architecture** (arXiv:2507.18178, July 2025) — knowledge retrieval localizes in lower layers; reasoning operates in higher layers. Models stripped of knowledge-pretraining pressure perform better on calibration and reasoning at fixed parameter budgets.
- **Knowledge is capacity-hungry, reasoning is data-hungry** (ACL 2025 / arXiv 2503.10061) — at fixed VRAM, more capacity goes to knowledge compression than to reasoning depth.
- **Persona drift is mechanistically real** (arXiv:2402.10962 + Lu et al. Jan 2026) — 20-40% identity-axis drift documented past turn 8-15 in 27B-70B class models. Late-position re-injection compensates for the attention-decay cause.
- **Reflection cycles crystallize observation into self-knowledge** (Park et al. 2023, "Generative Agents") — periodic reflection over recent observations produces higher-order insights that stabilize agent identity across long arcs.

The mechanics in this design address those mechanistic causes.

### Relationship to R716 (knowledge-sharing)

R716 designed a four-phase rollout for cross-agent knowledge sharing: visibility (Phase 1), subscription (Phase 2), verification (Phase 3), federated recall (Phase 4). This work absorbs **R716 Phase 1** (visibility as a fact property: `Private | Shared | Restricted | Published`) into the same lock-step crate move, because the visibility model is required for the private-workspace fence to work cleanly. **R716 Phase 3** (multi-agent verification + tier promotion) is tracked as a separate follow-up — load-bearing for fleet-wide knowledge confidence but not required for the private-nous use case. **R716 Phase 2 + Phase 4** are deferred; no consumer demand.

### Mechanics

#### 1. Recall config extensions

Four new fields on `RecallSettings` (taxis) and the corresponding `RecallConfig` (nous), with `From<taxis::config::RecallSettings>` propagating:

| Field | Type | Behaviour |
|---|---|---|
| `pinned_facts` | `Vec<FactId>` | Always-injected, slot-reserved at the top of the recall block. Bypasses scoring. |
| `late_inject_anchor` | `bool` | When true, also inject the pinned facts as a system message at end-of-context (split-softmax compensation, per arXiv:2402.10962). |
| `scope_quotas` | `HashMap<MemoryScope, usize>` | Per-scope minimums with slack-fill. Guarantees fair representation across scopes regardless of pure score ranking. |
| `reranker_url` | `Option<String>` | When set, route recall candidates through a cross-encoder reranker (HTTP). Falls back to the existing `NaiveReranker` when `None`. |

Tier weighting in scoring already exists (`score_epistemic_tier` weighting `Verified=1.0 / Inferred=0.6 / Assumed=0.3` plus `Training=4.0` stability multiplier). The new fields layer on top without touching scoring math.

`HttpReranker` lands as a new `Reranker` impl in `episteme::recall::reranker` alongside the existing `NaiveReranker`. The trait is `#[async_trait]` and stable; the new impl brings a small HTTP client behind the existing `reranker` feature flag.

#### 2. Extraction config extensions

Three new fields on `ExtractionConfig` (episteme):

| Field | Type | Behaviour |
|---|---|---|
| `extract_self_facts` | `bool` (default `true`) | When `false`, the architectural backstop in `extract_refined` rejects facts whose subject matches `"I"` / the agent's identity. Prevents the self-reinforcing meta-fact loop where the model says "I value X" → extraction captures → recall surfaces → reinforces. |
| `events_only_prompt` | `bool` (default `false`) | When `true`, the extraction prompt explicitly forbids meta-relational and self-descriptive fact patterns. |
| `default_tier` | `EpistemicTier` (default `Inferred`) | Replaces the hardcoded `Inferred` in `persist`. Lets profile bundles (below) raise extraction defaults if appropriate. |

#### 3. Reflection cycle (new pipeline stage)

A new `run_reflection_stage` async fn in `nous::pipeline::stages`, inserted after `run_finalize_stage` and before the training-capture block. Reads recent session facts from the per-nous knowledge store; promotes raw `FactType::Observation` records to a higher tier (existing `EpistemicTier::Verified` or a new `Reflected` variant — `EpistemicTier` is `#[non_exhaustive]`, so adding a variant is additive and safe). Config-gated by `reflection_enabled: bool` on `NousConfig` or `PipelineConfig`. New `StageBudget::reflection_secs: u32` for time-boxing.

This is the canonical tier-promotion path beyond the multi-agent verification mechanism that R716 Phase 3 will add.

#### 4. RecallProfile bundle

A new enum on `NousConfig`:

```rust
pub enum RecallProfile {
    Default,
    Archival,
    IdentityContinuity,
}
```

`IdentityContinuity` bundles the field changes above into a single config switch:

- `late_inject_anchor = true`
- `pinned_facts` slot-reserved top-3
- `scope_quotas` favoring user/relationship-scoped facts
- `extract_self_facts = false`
- `reflection_enabled = true`

Helper `RecallProfile::apply(&self, recall: &mut RecallConfig, extraction: &mut ExtractionConfig, pipeline: &mut PipelineConfig)` mutates the resolved configs at actor-build time.

#### 5. CrossNousRouter address mask

A new fence on inbound routing:

```rust
pub enum AddressMask {
    Public,
    OperatorOnly,
    Whitelist(Vec<String>),
}
```

`CrossNousRouter` gains `address_mask: HashMap<String, AddressMask>`; `send` and `ask` check the mask before delivering to the target inbox. Outbound is unaffected — a private nous can still send messages out (e.g. delegate research) without becoming addressable. Rejection returns `AddressRejectedSnafu` (or extends `NousNotFoundSnafu`).

#### 6. Per-nous episteme keyspace

`KnowledgeStore::open_fjall` already takes a path; the API stays stable, but the call sites that currently open a single shared store under `oikos.knowledge_db()` must scope by cohort:

- Default cohort `"shared"` preserves existing behaviour.
- Cohort override → `oikos.knowledge_db().join(cohort)`.
- `NousManager::knowledge_store` shifts from `Option<Arc<KnowledgeStore>>` to a per-nous map (or open-on-demand).

Production call sites needing updates: `aletheia::runtime::setup::open_knowledge_store`, the agent IO commands, the memory commands, the REPL, and `aletheia-memory-mcp::server`. Migration on first boot copies existing facts into the `shared` cohort path.

#### 7. Private workspace flag

Two design options:

- **Option A (lighter)**: Add `private: bool` to `taxis::config::NousDefinition` and propagate through `ResolvedNousConfig`. Mixes runtime config with workspace files but no new file format.
- **Option B (cleaner separation)**: Add a per-nous manifest reader to `BootstrapAssembler` that reads `instance/nous/<id>/manifest.toml`. Keeps the flag close to the workspace files it governs.

Either way, the flag drives:

- `BootstrapAssembler::resolve_workspace_files` — skip cross-nous discovery sources when `private == true`.
- `NousManager::list` — filter private nouses from public status output.
- `diaporeia::tools::nous_list` — filter from non-operator callers.
- `pylon::handlers::nous::list_nous` — filter at the HTTP API.

Recommendation: **Option A** for first land (smaller surface), with the option to migrate to a per-nous manifest later if the workspace flag proliferates.

#### 8. Tier-aware data model (cross-cutting)

Three orthogonal axes already exist or land here:

| Axis | Crate | Status | Purpose |
|---|---|---|---|
| `EpistemicTier` | `eidos::knowledge::fact` | Already present (`Verified | Inferred | Assumed | Training`) | Confidence-of-truth weighting |
| `FactSensitivity` | `eidos::knowledge::fact` | Already present (`Public | Internal | Confidential`) | Provider deployment / data-sovereignty filtering |
| `MemoryScope` | `eidos::knowledge` | Already present | Filesystem path scoping |
| `Visibility` | `eidos::knowledge::fact` | **NEW** (`Private | Shared | Restricted | Published`) | Cross-nous discoverability (R716 Phase 1) |

These axes do not interact at scoring; they layer in different filter passes. `Visibility` is consumed by recall to filter out other nouses' private facts; `FactSensitivity` is consumed earlier for sovereignty-tier filtering.

Schema migration: CozoDB does not support `ALTER`, so adding `visibility` to the `facts` relation requires a Datalog rebuild via the existing migration ladder (`init_schema`, currently at `SCHEMA_VERSION = 9`). A v10 migration rebuilds `facts` with the new column and backfills existing rows to `Private`.

### Cross-crate lock-step

The four crates must move together; partial merges break compilation because the config structs deserialize from a single TOML source:

- `eidos` — add `Visibility` enum; additive `EpistemicTier::Reflected` if used.
- `episteme` — propagate `visibility` into `ScoredResult`; add `HttpReranker`; extend `ExtractionConfig`.
- `nous` — consume new `episteme` types in `RecallStage`, `PipelineConfig`, `NousConfig`; add reflection stage; add address mask; per-nous keyspace.
- `taxis` — extend `RecallSettings`, `NousDefinition`, `ResolvedNousConfig`.

## Recommendations

1. **Land in the order**: eidos additive types → episteme propagation → nous integration → taxis config wiring. Tests at every layer; the integration test in the final crate exercises the full pipeline.

2. **Use `RecallProfile::IdentityContinuity` as the canonical bundle**. Operators configuring a private nous set a single field; the bundle does the rest. Avoids cargo-culting the individual flags across multiple workspaces.

3. **Reflection promotes, never overwrites**. Source observation facts retain their original tier; reflection emits new facts that reference the sources via existing provenance fields.

4. **Recommend Option A for the private flag** (in `NousDefinition`). Smaller surface; can migrate to a per-nous manifest later if it becomes load-bearing.

5. **Default cohort `"shared"` for per-nous keyspace**. Preserves existing behaviour without forcing every existing nous to declare a cohort.

6. **Schema v10 migration is operator-blocking until run**. Communicate the migration in the release notes; provide a `--migrate-only` mode if the migration takes meaningful time.

## Gotchas

- **MemoryScope vs. Visibility naming collision.** `Fact` already has `scope: Option<MemoryScope>`. `Visibility` is a different axis. The extraction pipeline must not conflate them when emitting facts.

- **`extract_refined` line 575 hardcoded tier.** This is the existing default; the new `default_tier` config replaces it. Tests in `episteme::extract::tests::config_parsing` need updates.

- **R716 partial absorption.** R716's `published_facts` and `provenance` relations are **not** added by this work. Adding them later (under R716 Phase 3) does not break what this work lands; it adds rows to a new relation rather than mutating the existing one.

- **Stages ordering for reflection.** The reflection stage runs after `run_finalize_stage` (which persists the user-visible turn) but before training-capture. If reflection writes new facts to the knowledge store, training capture sees them; if it writes only to a separate reflection sink, training capture does not. Decide explicitly and test.

- **`#[non_exhaustive]` on `EpistemicTier` is a contract.** Downstream pattern matches must use `_ => ...` arms. A new `Reflected` variant for the reflection cycle is safe; downstream consumers compile without change because they already handle the `_` case (or fail their own lints).

- **`AddressMask` enforcement is inbound-only.** A private nous can still ask others to do work on its behalf. The receiving nous's outbound logs are a side channel — the operator is the trust boundary, but if a private nous delegates to a less-private nous, the receiving nous's extracted facts about the request are themselves private-leakage surface. Document the asymmetry; no enforcement at this layer.

- **CozoDB v10 migration is one-way.** Backups of `data/knowledge.fjall/` should be taken before the migration runs in production. The existing migration ladder writes a backup; ensure the v10 migration preserves that pattern.

## References

- `crates/eidos/src/knowledge/fact.rs` — `Fact`, `EpistemicTier`, `FactProvenance`, `FactSensitivity`, `FactAccess`, `MemoryScope`
- `crates/episteme/src/recall/mod.rs` — `RecallEngine`, `score_epistemic_tier`, `RecallWeights`
- `crates/episteme/src/recall/reranker.rs` — `Reranker` trait, `NaiveReranker`
- `crates/episteme/src/extract/engine.rs` — `extract_refined`, `persist`, prompt construction
- `crates/episteme/src/extract/types.rs` — `ExtractionConfig`
- `crates/episteme/src/knowledge_store/mod.rs` — `KnowledgeStore`, `open_fjall`, `init_schema`, migration ladder
- `crates/nous/src/recall/scoring.rs` — `RecallConfig`
- `crates/nous/src/recall/mod.rs` — `RecallStage`, `finalize_results`, `filter_by_sensitivity`
- `crates/nous/src/pipeline/mod.rs` — pipeline stage ordering
- `crates/nous/src/pipeline/stages.rs` — `run_recall_stage`, `apply_recall_result`, `run_finalize_stage`
- `crates/nous/src/cross/router.rs` — `CrossNousRouter`, `send`, `ask`
- `crates/nous/src/bootstrap/mod.rs` — `BootstrapAssembler`, `resolve_workspace_files`
- `crates/nous/src/manager.rs` — `NousManager`, actor spawn path
- `crates/nous/src/config.rs` — `NousConfig`, `PipelineConfig`, `StageBudget`
- `crates/taxis/src/config/agents.rs` — `RecallSettings`, `NousDefinition`
- `crates/taxis/src/config/resolved.rs` — `ResolvedNousConfig`, `resolve_nous`
- `crates/diaporeia/src/tools/mod.rs` — `nous_list`
- `crates/pylon/src/handlers/nous.rs` — HTTP `list_nous`
- `crates/aletheia/src/runtime/setup.rs` — `open_knowledge_store`
- R716 (`knowledge-sharing.md`) — visibility model, provenance schema, verification protocol (Phase 3 deferred)
- R717 (`active-forgetting.md`) — FSRS decay; tier-aware stability multipliers
- arXiv:2402.10962 — Persona drift mechanism
- Park et al. 2023 — Generative Agents reflection cycle pattern
- arXiv:2507.18178, arXiv:2503.10061 — Cognition-vs-knowledge separability empirical findings
