# mneme

## At a glance

Curated facade re-exporting memory and session types from sub-crates. Depends on eidos, graphe, and episteme. Entry point: `src/lib.rs` (SessionStore, KnowledgeStore).

## Depth

Curated facade re-exporting from four decomposed sub-crates. ~390 lines of glue code.

## Facade justification (#3243)

Decision: **keep mneme**. Evaluated per STANDARDS.md "Everything must earn its place."

1. **API stability**: 7+ downstream crates (nous, pylon, aletheia, melete, daemon, diaporeia, integration-tests) import from `mneme`. If sub-crates are reorganized, downstream `use` statements do not change.
2. **Feature gating**: mneme gates krites behind `mneme-engine`. Without the facade, every consuming crate would duplicate this feature gate.
3. **Import ergonomics**: single `mneme::` prefix replaces four crate prefixes.

**Alarm threshold**: if `lib.rs` exceeds 500 lines, the facade is accreting logic that belongs in a sub-crate. Audit and extract.

## Architecture

Mneme was decomposed into eidos, graphe, episteme, and krites. This crate re-exports only the types that downstream consumers actually import, not entire modules. Internal types remain accessible through the sub-crates directly.

## Re-exports

Only types with downstream consumers are surfaced. Modules not listed here (admission, conflict, decay, vocab, knowledge_portability, query) are internal to episteme/graphe and not re-exported.

| Source crate | Re-exported modules | Key types | Feature gate |
|--------------|---------------------|-----------|--------------|
| `eidos` | `id`, `knowledge` | (full modules) | always |
| `graphe` | `error` | `Error` | always |
| `graphe` | `portability` | `AgentFile`, export metadata, omitted/truncated section records, workspace binary payloads, session/message/knowledge export DTOs | always |
| `graphe` | `store` | `SessionStore`, finalize-turn request/result DTOs | always |
| `graphe` | `types` | `Message`, `Role`, `Session`, `SessionMetrics`, `SessionOrigin`, `SessionStatus`, `SessionType`, `ToolAuditRecord`, `UsageRecord`, validated session/agent ID helpers | always |
| `episteme` | `consolidation` | `ConsolidationConfig` | always |
| `episteme` | `embedding` | `EmbeddingProvider`, `DegradedEmbeddingProvider`, `EmbeddingConfig`, `EmbeddingError`, `create_provider`, `is_degraded_provider`, `MockEmbeddingProvider` (test-support) | always |
| `episteme` | `embedding_eval` | `EvalDataset`, `EvalRunResult`, `compare_models` | always |
| `episteme` | `extract` | `ConversationMessage`, `ExtractionConfig`, `ExtractionEngine`, `ExtractionError`, `ExtractionProvider`, `ExtractedToolCall`, `LlmCallSnafu` | always |
| `episteme` | `instinct` | `ToolObservation`, `ToolOutcome`, `sanitize_parameters`, `truncate_context_summary`, constants | always |
| `episteme` | `knowledge_store` | `HybridQuery`, `KnowledgeConfig`, `KnowledgeStore`, `QueryResult` | `mneme-engine` |
| `episteme` | `recall` | `FactorScores`, `RecallEngine`, `RecallWeights`, `ScoredResult` | always |
| `episteme` | `skill` | `SkillContent`, `export_skills_to_cc`, `parse_skill_md`, `scan_skill_dir` | always |
| `episteme` | `skills` | `CandidateTracker`, `ContentEvidenceRef`, `ExtractedSkill`, `PendingSkill`, `SkillCandidate`, `SkillExtractionAudit`, `SkillExtractor`, `SkillObservationEvidence`, `SkillReviewAudit`, `SkillReviewDecision`, `SkillReviewInput`, `SkillSourceEvidence`, `ToolCallRecord`, `TrackResult`, `tool_sequence_hash` + `extract` submodule | always |
| `episteme` | `manifest`, `query_rewrite`, `side_query`, `trace_ingest`, `verification` | public support modules used by recall, tracing, and verification consumers | always |
| `episteme`/`graphe` | `metrics` | extraction/embedding/conflict/correction quality metrics, `record_backup_duration`, `register_knowledge`, `register_sessions` | always |
| local | `checkpoint`, `finalize` | working-checkpoint store contract and idempotent turn-finalization primitives | always |
| local | `benchmark` | benchmark isolation and evidence primitives | `mneme-engine` |
| `eidos` | `bookkeeping` | provider contracts and extraction DTOs | always |
| `eidos` | `training` | `TrainingConfig`, `TrainingRecord`, `TRAINING_RECORD_SCHEMA_VERSION` | always |
| `krites` | `engine` | (full crate alias) | `mneme-engine` |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `graph-algo` | yes | Graph algorithms in episteme + krites |
| `mneme-engine` | yes | Datalog engine (krites) + typed query builder |
| `storage-fjall` | no | Fjall LSM-tree backend for the knowledge store (requires mneme-engine) |
| `embed-candle` | no | Local ML embeddings via candle |
| `test-support` | no | MockEmbeddingProvider and test helpers |

## Training capture

The capture logic (`TrainingCapture`, `CaptureInput`, `CaptureStopReason`, quality gate) lives in `nous::training`, not here. Mneme re-exports the shared types (`TrainingConfig`, `TrainingRecord`) from eidos. Training runs as a pipeline tap (side-effect observer on the turn loop), not a memory operation.

## Where to make changes

Mneme itself has no logic. All changes go to the sub-crates:

| Task | Sub-crate |
|------|-----------|
| Add session/message field | `graphe` (types, store) |
| Add knowledge type | `eidos` (knowledge module) |
| Add extraction/recall logic | `episteme` |
| Add Datalog query builder | `episteme` (query module, requires mneme-engine) |
| Modify Datalog engine | `krites` |
| Add embedding provider | `episteme` (embedding module) |

## Recent substrate notes

- The facade boundary is intentional: downstream application crates should import memory/session/recall/trace types through `mneme`.
- `TraceIngestLayer`, side-query, visibility filtering, and knowledge-store helpers are re-exported only when they have downstream consumers.
- Add explicit re-exports for new downstream needs instead of reaching around the facade.

## Dependencies

Uses: eidos, graphe, episteme, krites
Used by: nous, pylon, aletheia, diaporeia, integration-tests
