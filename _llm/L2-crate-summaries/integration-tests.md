# integration-tests

**Purpose:** Cross-crate integration test suite exercising multi-crate pipelines end-to-end. Test-only crate; no library code.

## Key types

| File | Coverage |
|------|---------|
| `end_to_end.rs` | HTTP → pipeline → provider → persistence with mock LLM |
| `mneme_session.rs` | SessionStore CRUD across crate boundaries |
| `recall_pipeline.rs` | Recall with MockEmbeddingProvider and mock vector search |
| `knowledge_engine.rs` | CozoDB knowledge store operations (requires `engine-tests` feature) |
| `knowledge_lifecycle.rs` | Fact insertion, recall, consolidation lifecycle |

## Public API surface

- No library exports — all integration test files under `tests/`
- Run with `cargo test -p integration-tests` or `cargo test --workspace --features test-full`

## When to look here

- When verifying a cross-crate refactor didn't break end-to-end pipelines
- When adding integration coverage for a new multi-crate interaction
