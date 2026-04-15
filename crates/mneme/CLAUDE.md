# mneme

Curated facade re-exporting from four decomposed sub-crates. ~270 lines of glue code.

## Architecture

Mneme was decomposed into eidos, graphe, episteme, and krites. This crate re-exports only the types that downstream consumers actually import, not entire modules. Internal types remain accessible through the sub-crates directly.

## Re-exports

Only types with downstream consumers are surfaced. Modules not listed here (admission, conflict, decay, vocab, knowledge_portability, query, migration, recovery, retention, schema) are internal to episteme/graphe and not re-exported.

| Source crate | Re-exported modules | Key types | Feature gate |
|--------------|---------------------|-----------|--------------|
| `eidos` | `id`, `knowledge` | (full modules) | always |
| `graphe` | `backup` | `BackupManager` | `sqlite` |
| `graphe` | `error` | `Error` | always |
| `graphe` | `export` | `ExportOptions`, `export_agent` | `sqlite` |
| `graphe` | `import` | `ImportOptions`, `import_agent` | `sqlite` |
| `graphe` | `portability` | `AgentFile` | `sqlite` |
| `graphe` | `store` | `SessionStore` | `sqlite` or `fjall` |
| `graphe` | `types` | `Message`, `Role`, `Session`, `SessionMetrics`, `SessionOrigin`, `SessionStatus`, `SessionType`, `UsageRecord` | always |
| `episteme` | `consolidation` | `ConsolidationConfig` | always |
| `episteme` | `embedding` | `EmbeddingProvider`, `DegradedEmbeddingProvider`, `EmbeddingConfig`, `create_provider`, `MockEmbeddingProvider` (test-support) | always |
| `episteme` | `embedding_eval` | `EvalDataset`, `EvalRunResult`, `compare_models` | always |
| `episteme` | `extract` | `ConversationMessage`, `ExtractionConfig`, `ExtractionEngine`, `ExtractionError`, `ExtractionProvider`, `LlmCallSnafu` | always |
| `episteme` | `instinct` | `ToolObservation`, `ToolOutcome`, `sanitize_parameters`, `truncate_context_summary`, constants | always |
| `episteme` | `knowledge_store` | `HybridQuery`, `KnowledgeConfig`, `KnowledgeStore` | always |
| `episteme` | `recall` | `FactorScores`, `RecallEngine`, `RecallWeights`, `ScoredResult` | always |
| `episteme` | `skill` | `SkillContent`, `export_skills_to_cc`, `parse_skill_md`, `scan_skill_dir` | always |
| `episteme` | `skills` | `CandidateTracker`, `PendingSkill`, `SkillExtractor`, `ToolCallRecord`, `TrackResult` + `extract` submodule | always |
| `krites` | `engine` | (full crate alias) | `mneme-engine` |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `sqlite` | yes | SQLite session store (graphe backend) |
| `graph-algo` | yes | Graph algorithms in episteme + krites |
| `mneme-engine` | yes | Datalog engine (krites) + typed query builder |
| `storage-fjall` | no | Fjall LSM-tree backend (requires mneme-engine) |
| `embed-candle` | no | Local ML embeddings via candle |
| `hnsw_rs` | no | Alternative HNSW vector index backend |
| `test-support` | no | MockEmbeddingProvider and test helpers |

## Where to make changes

Mneme itself has no logic. All changes go to the sub-crates:

| Task | Sub-crate |
|------|-----------|
| Add session/message field | `graphe` (types, schema, migration, store) |
| Add knowledge type | `eidos` (knowledge module) |
| Add extraction/recall logic | `episteme` |
| Add Datalog query builder | `episteme` (query module, requires mneme-engine) |
| Modify Datalog engine | `krites` |
| Add embedding provider | `episteme` (embedding module) |

## Dependencies

Uses: eidos, graphe, episteme, krites
Used by: nous, pylon, melete, aletheia (binary)
