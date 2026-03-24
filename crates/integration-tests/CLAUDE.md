# integration-tests

Cross-crate integration test suite exercising multi-crate pipelines end-to-end. 5.7K lines. No library code (test-only crate).

## Read first

1. `tests/end_to_end.rs`: HTTP -> pipeline -> provider -> persistence full stack tests
2. `tests/mneme_session.rs`: SessionStore CRUD across crate boundaries
3. `tests/recall_pipeline.rs`: Recall with mock vectors end-to-end
4. `tests/knowledge_engine.rs`: Knowledge store Datalog integration
5. `tests/pipeline_assembly.rs`: Nous pipeline stage assembly tests

## Test files

| File | Tests |
|------|-------|
| `end_to_end.rs` | HTTP request through Pylon router to nous pipeline with mock LLM |
| `mneme_session.rs` | Session creation, message append, history retrieval via SessionStore |
| `recall_pipeline.rs` | Recall stage with MockEmbeddingProvider and mock vector search |
| `recall_scoring.rs` | Multi-factor recall scoring across knowledge/nous boundaries |
| `knowledge_engine.rs` | CozoDB knowledge store operations (requires `engine-tests`) |
| `knowledge_lifecycle.rs` | Fact insertion, recall, consolidation lifecycle |
| `knowledge_recall.rs` | Knowledge recall with embedding similarity |
| `cross_crate_pipeline.rs` | Multi-crate pipeline stage integration |
| `pipeline_assembly.rs` | Nous pipeline context assembly |
| `organon_mneme_tools.rs` | Tool executor -> session store interactions |
| `hermeneus_from_mneme.rs` | LLM provider used with session persistence |
| `eval_harness.rs` | Eval scenario framework integration |
| `access_tracking.rs` | Knowledge access tracking across crates |
| `domain_packs.rs` | Domain pack loading and validation |
| `oikos_cascade.rs` | Workspace cascade resolution |
| `engine_facade.rs` | Krites engine facade integration |
| `nous_session_state.rs` | Nous session state management |

## Feature flags

| Feature | Default | Purpose |
|---------|---------|---------|
| `sqlite-tests` | yes | Tests requiring SessionStore (SQLite) |
| `engine-tests` | no | Tests requiring CozoDB Datalog engine |
| `knowledge-store` | yes | Knowledge store feature propagation to nous/pylon |

## Patterns

- **Mock providers**: tests use `MockProvider` (hermeneus), `MockEmbeddingProvider` (mneme) for deterministic LLM/embedding responses.
- **In-memory stores**: `SessionStore::open_in_memory()` avoids disk I/O in tests.
- **Feature-gated modules**: each test file declares `#![cfg(feature = "...")]` for appropriate test tier.

## Common tasks

| Task | Where |
|------|-------|
| Add cross-crate test | New file in `tests/`, add feature gate if needed |
| Add test dependency | `Cargo.toml` [dev-dependencies] |
| Enable feature for tests | `Cargo.toml` [features] section |

## Dependencies

Uses (dev): dokimion, koina, taxis, mneme, hermeneus, nous, organon, thesauros, pylon, symbolon, axum, reqwest, tokio, tower
Used by: (none -- test-only crate)
