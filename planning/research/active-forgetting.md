# R717: Active forgetting system

## Question

Memory in mneme only grows. The `is_forgotten` flag exists for user-directed and supersession-based forgetting, but no automated process identifies and retires stale, contradicted, or low-value memories. How should the system actively forget, and what mechanisms keep the knowledge base relevant as it scales?

## Findings

### Current memory lifecycle

Facts flow through a linear pipeline: extraction, conflict detection, storage, recall, and (occasionally) manual forgetting. No automated process closes the loop by retiring facts that have outlived their usefulness.

**Creation paths:**

| Path | Trigger | Storage |
|---|---|---|
| Conversation extraction | Every turn via LLM | CozoDB `facts` relation |
| Conflict resolution | Contradiction detected | Supersedes old fact, inserts new |
| Entity dedup | Merge score > 0.90 | Transfers facts, records audit |
| Consolidation | Entity exceeds 10 facts | LLM summarizes, inserts consolidated |

**Existing cleanup mechanisms:**

| Mechanism | Scope | Automated? |
|---|---|---|
| Session retention | SQLite sessions older than 90 days | Yes (periodic) |
| Orphan message cleanup | Messages with no session, 30 days | Yes (periodic) |
| Supersession | Old fact gets `valid_to = now` | Yes (on conflict) |
| Soft-delete (`is_forgotten`) | Individual facts | Manual only |
| Consolidation | Entity fact count > 10 | Semi-auto (rate-limited) |
| Blackboard TTL | Key-value entries, 1 hour default | Yes (per-key) |

**Gap:** Session retention removes conversation history but leaves extracted facts orphaned in CozoDB. A user's first conversation may produce 50 facts. After 90 days, the session is gone, but all 50 facts persist indefinitely. Nothing evaluates whether those facts are still true, relevant, or worth the storage and recall noise.

### Bi-temporal model

The `Fact` struct tracks two temporal dimensions:

1. **Valid time** (`valid_from`, `valid_to`): when the fact was true in the real world
2. **Transaction time** (`recorded_at`): when the system learned about it

Supersession chains link old facts to replacements via `superseded_by`. This is correct. "Forgetting" in a bi-temporal system does not mean the fact never existed. It means the system records when the knowledge became invalid. The `valid_to` timestamp already captures this for contradicted facts. What's missing is the mechanism that sets `valid_to` for facts that become stale through inaction rather than contradiction.

The `is_forgotten` flag operates outside the temporal model. It is a query-time filter, not a temporal assertion. A forgotten fact is invisible to recall but temporally unchanged. This is the right design: forgetting is an access-control decision, not a truth claim.

### FSRS decay and recall scoring

The recall engine already models memory decay via FSRS power-law:

```
R(t) = (1 + 19/81 * t/S)^(-0.5)
```

Where `S` = base stability (72h for observations, 17,520h for identity) multiplied by epistemic tier (2x for verified, 0.5x for assumed) and volatility adjustment (0.5x to 1.5x).

Decay affects **ranking**, not **existence**. A fact with R(t) = 0.01 still appears in recall results if no better candidates exist. The system never transitions a low-decay-score fact to forgotten status. This is the core gap.

### Storage growth analysis

Without active forgetting, fact count grows monotonically. Consolidation compresses but does not remove. Supersession marks old facts but adds new ones. Over time:

- Active facts: O(conversations * extraction_rate)
- Superseded facts: accumulate as chains grow
- Forgotten facts: near zero (manual only)
- Embeddings: one per fact, never pruned
- Graph nodes/edges: entities merge but never delete

For a moderately active user (10 conversations/week, 5 facts/conversation), the system accumulates ~2,600 facts/year before consolidation. Consolidation compresses at roughly 2:1 after the 10-fact threshold, yielding ~1,500 active facts/year. After 3 years: ~4,500 active facts, ~3,300 superseded/consolidated, ~7,800 total records, plus one embedding per record.

This is not a storage crisis. CozoDB handles millions of records. The problem is recall quality: as fact count grows, the signal-to-noise ratio in recall results degrades. Low-quality facts dilute the top-k results even when ranked low.

### Forgetting strategies evaluated

#### 1. decay-threshold forgetting

Transition facts to `is_forgotten` when their FSRS decay score falls below a threshold for a sustained period.

| Factor | Assessment |
|---|---|
| Mechanism | Background sweep checks R(t) < threshold (e.g., 0.05) for all active facts |
| Threshold | Must be per-FactType: identity facts at R=0.05 are 40+ years old, observations at R=0.05 are 2 weeks old |
| Grace period | Require R(t) < threshold for N consecutive checks (e.g., 3 sweeps) to avoid premature forgetting during usage gaps |
| Strength | Aligns with existing FSRS model. No new signals needed. |
| Weakness | Penalizes niche-but-valuable facts that are rarely recalled. A fact about a user's rare allergy is critical but low-access. |
| Mitigation | Exempt `verified` tier facts. Use fact_type-specific thresholds. Allow user-pinned facts. |

**Verdict: Primary mechanism.** FSRS already models relevance decay. The missing piece is an actuator that converts low scores to forgotten status.

#### 2. contradiction-based forgetting

When conflict detection identifies a CONTRADICTS classification, the old fact is superseded. This already works. The gap is detecting contradictions in facts that were never explicitly contradicted but are implausible given newer information.

| Factor | Assessment |
|---|---|
| Mechanism | Periodic LLM pass over high-staleness facts: "Given what we now know, is this still true?" |
| Strength | Catches implicit contradictions that extraction-time conflict detection misses |
| Weakness | Expensive (LLM call per candidate batch). False positives risk losing good facts. |
| Mitigation | Only evaluate candidates already flagged by decay-threshold (R(t) < 0.15). Batch candidates into single LLM calls. |

**Verdict: Secondary mechanism, gated behind decay threshold.** Only evaluate facts that are already fading. This keeps LLM costs proportional to decay, not total fact count.

#### 3. relevance-based forgetting (access frequency)

Facts never recalled are candidates for removal. `access_count` already tracks this.

| Factor | Assessment |
|---|---|
| Mechanism | Facts with `access_count = 0` and `age > N days` are candidates |
| Strength | Direct signal: if the system never needed this fact, it probably never will |
| Weakness | Cold-start problem: new facts have zero access by definition. Conflates "not yet relevant" with "never relevant." |
| Mitigation | Minimum age gate (e.g., 60 days with zero access). Combine with decay score, not standalone. |

**Verdict: Contributing signal within decay scoring, not standalone trigger.** Access frequency already has 5% weight in recall. Bump to 10% or add a zero-access penalty to the decay formula.

#### 4. confidence-based forgetting

Low-confidence extractions (`confidence < 0.5`, tier = `assumed`) are pruned first.

| Factor | Assessment |
|---|---|
| Mechanism | Periodic sweep: forget facts where `confidence < threshold AND tier = assumed` |
| Strength | Targets the lowest-quality extractions. Aligns with epistemic model. |
| Weakness | Confidence is set at extraction time and never updated. A 0.4-confidence fact that's been accessed 50 times is probably valuable despite its initial score. |
| Mitigation | Combine with access count: only prune low-confidence facts with low access. |

**Verdict: Pre-filter for decay-threshold candidates.** Low-confidence, low-access assumed facts should have the lowest effective stability, making them the first to cross the decay threshold naturally. Adjust the FSRS stability formula to penalize low-confidence facts more aggressively rather than adding a separate mechanism.

#### 5. user-directed forgetting

Already implemented via `forget_fact(reason: ForgetReason::UserRequested)`. Working correctly. No changes needed except surface it better in the UI/API.

### What will NOT work

**Hard deletion.** Physically removing facts from CozoDB destroys the audit trail and breaks supersession chains. A deleted fact referenced by `superseded_by` creates dangling pointers. The bi-temporal model depends on historical records existing. Hard deletion is only appropriate for GDPR-style "right to be forgotten" requests, and even then, the audit record should persist with content redacted rather than removed.

**Global TTL.** A single time-to-live for all facts is wrong. Identity facts ("user's name") should persist for years. Task facts ("need to fix bug X") should decay in days. The FactType-based stability already models this correctly. A global TTL would either expire valuable facts or retain garbage.

**Aggressive consolidation as forgetting.** Consolidation compresses multiple facts into summaries but does not reduce recall noise. A consolidated fact is still a fact. Over-consolidating loses specificity without reducing count.

**Real-time forgetting on every recall.** Evaluating forgetting candidates during recall adds latency to the hot path. Forgetting must be a background process, not inline.

## Recommendations

### Architecture: background forgetting sweep

A periodic background task (the "forgetting sweep") runs on a configurable schedule (default: daily) and processes facts through a multi-stage pipeline:

```
Stage 1: Candidate identification
  - Compute R(t) for all active, non-verified facts
  - Select candidates where R(t) < forget_threshold (configurable per FactType)
  - Exclude user-pinned facts

Stage 2: Grace period check
  - Require candidate to have been below threshold for N consecutive sweeps
  - Track sweep history in a `forget_candidates` relation (fact_id, first_flagged_at, sweep_count)
  - Default: 3 consecutive sweeps (3 days at daily frequency)

Stage 3: LLM validation (optional, for high-value facts)
  - Facts with access_count > 10 OR tier = inferred get LLM review before forgetting
  - Batch up to 20 candidates per LLM call
  - LLM classifies: FORGET (confirm), KEEP (reset grace period), SUPERSEDE (generate replacement)

Stage 4: Execution
  - Set is_forgotten = true, forgotten_at = now
  - Set forget_reason = ForgetReason::Stale (or ::Outdated if LLM says contradicted)
  - Log to forget_audit relation (fact_id, reason, sweep_id, llm_reviewed)
```

### Data model additions

**New CozoDB relation: `forget_candidates`**

```
forget_candidates {
    fact_id: String =>
    first_flagged_at: String,
    sweep_count: Int,
    last_decay_score: Float,
    last_swept_at: String
}
```

**New CozoDB relation: `forget_audit`**

```
forget_audit {
    id: String =>
    fact_id: String,
    reason: String,
    decay_score_at_forget: Float,
    llm_reviewed: Bool,
    llm_verdict: String?,
    sweep_id: String,
    forgotten_at: String
}
```

**New field on `Fact`: `is_pinned: bool`**

User-pinned facts are exempt from automated forgetting. Pinning is explicit: "never forget this." Default: false.

### Forgetting thresholds by factType

| FactType | Base stability (h) | Forget threshold (R) | Effective age at forget |
|---|---|---|---|
| Identity | 17,520 | 0.02 | ~40 years (effectively permanent) |
| Preference | 8,760 | 0.03 | ~15 years |
| Skill | 4,380 | 0.05 | ~3 years |
| Relationship | 2,190 | 0.05 | ~18 months |
| Event | 720 | 0.10 | ~3 months |
| Task | 168 | 0.15 | ~2 weeks |
| Observation | 72 | 0.15 | ~5 days |

These thresholds assume `tier = inferred` (1.0x multiplier) and zero access. Access bumps and verified tier push the effective age much higher.

### Soft delete, not hard delete

All forgetting uses `is_forgotten = true`. The fact remains in CozoDB for:

- Audit trail (what was forgotten and why)
- Supersession chain integrity (no dangling `superseded_by` references)
- Bi-temporal correctness (the fact existed; it was later found to be stale)
- Undo capability (clear `is_forgotten` to restore)

### Undo and safety

**Undo window:** Forgotten facts can be restored via `unforget_fact()` (already implemented). No time limit on undo. The audit trail in `forget_audit` provides the full history.

**Undo triggers:**
- User explicitly asks to recall a forgotten fact
- A new extraction contradicts the forgetting decision (the fact becomes relevant again)
- Admin review of the forget_audit log

**Safety rails:**
- `verified` tier facts are never auto-forgotten (only manual or privacy)
- `is_pinned` facts are exempt
- Grace period prevents premature forgetting during usage gaps (vacations, project pauses)
- LLM review gate for frequently-accessed facts prevents losing proven-valuable knowledge
- The forgetting sweep logs every decision for post-hoc analysis

### Schedule and configuration

```rust
pub struct ForgetConfig {
    /// Whether automated forgetting is enabled. Default: true.
    pub enabled: bool,
    /// Sweep interval. Default: 24 hours.
    pub sweep_interval_hours: u32,
    /// Number of consecutive below-threshold sweeps before forgetting. Default: 3.
    pub grace_sweeps: u32,
    /// Whether to use LLM validation for high-value candidates. Default: true.
    pub llm_review_enabled: bool,
    /// Minimum access_count to trigger LLM review instead of auto-forget. Default: 10.
    pub llm_review_access_threshold: u32,
    /// Maximum candidates per LLM batch. Default: 20.
    pub llm_batch_size: usize,
    /// Per-FactType R(t) thresholds. Uses defaults if not specified.
    pub thresholds: HashMap<FactType, f64>,
}
```

### Embedding cleanup

When a fact is forgotten, its embedding in the HNSW index should be flagged for lazy removal. The HNSW index does not support true deletion, but the `query_forgotten_ids` post-filter already excludes forgotten facts from search results. On index rebuild (periodic maintenance), forgotten embeddings are excluded.

Recommended: track `embedding_count` and `forgotten_embedding_count`. When the ratio exceeds 20%, trigger an index rebuild.

### Integration with bi-temporal model

Automated forgetting does not set `valid_to`. The decay-threshold forgetting mechanism says "this fact is no longer useful to recall," not "this fact is no longer true." These are distinct claims:

- `valid_to` set: the fact is known to be false or superseded (truth claim)
- `is_forgotten` set: the fact is excluded from recall (access decision)

A forgotten fact with `valid_to` still at the epoch-max sentinel was true when recorded and may still be true. The system stopped surfacing it because it was never useful, not because it was wrong.

For LLM-reviewed facts classified as SUPERSEDE, both `valid_to` and `is_forgotten` should be set, and a new replacement fact should be created (standard supersession flow).

## Gotchas

1. **Vacation problem.** A user inactive for 30 days triggers decay on all their Event and Task facts. The grace period (3 sweeps = 3 days) is too short. Consider pausing sweeps when the nous has had zero sessions in the sweep interval, or scaling grace period by nous inactivity.

2. **Consolidation interaction.** Consolidated facts inherit the consolidation's `recorded_at` but may reference old `valid_from` dates. The decay formula uses `last_accessed_at` (or `recorded_at` if never accessed), so consolidated facts start with fresh decay clocks. This is correct but means consolidated facts will never be auto-forgotten shortly after consolidation, even if the original facts were stale. This is acceptable: consolidation already implies the facts were worth keeping.

3. **Index bloat.** HNSW indexes do not support true deletion. Forgotten facts waste index space and slow kNN search. The post-filter handles correctness, but a 50% forgotten ratio means half the kNN results are discarded. Monitor the ratio and rebuild when it gets high.

4. **LLM cost scaling.** The LLM review gate adds cost proportional to the number of high-value candidates crossing the decay threshold. For a mature system with 5,000+ facts, a mass threshold crossing (e.g., after a long inactivity period) could trigger hundreds of LLM calls. Cap per-sweep LLM budget and process remaining candidates in subsequent sweeps.

5. **Race condition with extraction.** If a forgetting sweep runs concurrently with fact extraction, a just-extracted fact could be evaluated before its first recall. The minimum-age gate (derived from FactType base stability) and the grace period together prevent this, but the sweep must check `recorded_at` to avoid evaluating facts younger than the sweep interval.

6. **Privacy forgetting vs. stale forgetting.** `ForgetReason::Privacy` should trigger content redaction (replace `content` with `[REDACTED]`), not just `is_forgotten = true`. A forgotten-but-readable privacy-sensitive fact in the audit trail defeats the purpose. This is a separate concern from decay-based forgetting but shares the mechanism.

7. **Cross-nous facts.** Facts extracted by one nous but relevant to another (shared knowledge) need careful handling. The forgetting sweep should only evaluate facts for the nous that owns them (`nous_id` field), not for other nous instances that might benefit from shared recall.

## Effort estimate

| Component | Effort | Notes |
|---|---|---|
| `forget_candidates` relation + schema | Small | New CozoDB relation, single DDL statement |
| `forget_audit` relation + schema | Small | New CozoDB relation |
| `is_pinned` field on `Fact` | Small | Schema change, migration, API surface |
| `ForgetConfig` struct | Small | Configuration, defaults, validation |
| Forgetting sweep (stages 1-2) | Medium | Datalog queries + grace period logic |
| LLM review gate (stage 3) | Medium | Prompt engineering, batch processing |
| Execution + audit logging (stage 4) | Small | Existing `forget_fact()` + audit insert |
| Embedding ratio tracking + rebuild trigger | Medium | HNSW rebuild scheduling |
| API: pin/unpin facts | Small | Thin wrapper |
| Tests | Medium | Property tests for decay thresholds, integration tests for sweep pipeline |
| **Total** | **~2-3 prompts** | Split: schema/config, sweep pipeline, LLM review |

### Recommended implementation order

1. Schema additions (`forget_candidates`, `forget_audit`, `is_pinned`) and `ForgetConfig`
2. Decay-threshold sweep (stages 1, 2, 4) without LLM review
3. LLM review gate (stage 3) and embedding ratio monitoring
4. Pin/unpin API and any UI integration

Phase 1-2 delivers the core value. Phase 3 adds quality. Phase 4 adds user control.

## References

- `crates/mneme/src/knowledge.rs`: Fact, ForgetReason, FactType, EpistemicTier definitions
- `crates/mneme/src/knowledge_store/facts.rs`: `forget_fact()`, `unforget_fact()`, `list_forgotten()`
- `crates/mneme/src/recall.rs`: FSRS decay formula, 6-factor scoring, RecallWeights
- `crates/mneme/src/succession.rs`: Domain volatility, adaptive stability multiplier
- `crates/mneme/src/conflict.rs`: Contradiction detection, supersession
- `crates/mneme/src/consolidation/mod.rs`: LLM-driven fact compression
- `crates/mneme/src/retention.rs`: Session retention policy (SQLite only)
- `crates/mneme/src/knowledge_store/search.rs`: `query_forgotten_ids()` post-filter
- `crates/mneme/src/schema.rs`: Blackboard TTL (only existing TTL mechanism)
- FSRS algorithm: Piotr Wozniak's spaced repetition research, adapted for knowledge graph context
- Prompt 012 (`mneme-explicit-forgetting`): original soft-delete implementation
- Prompt 011 (`mneme-ecological-succession`): domain volatility tracking
- Prompt 006 (`mneme-fsrs-decay`): FSRS power-law decay scoring
