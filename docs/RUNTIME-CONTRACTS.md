# Runtime contracts

Aletheia describes and operates the same system through several surfaces —
CLI, TUI (koilon), desktop (proskenion), HTTP/SSE (pylon), docs, generated
`_llm` material, config, traces, and manifests (aletheia#4544). Each row below
names the ONE crate/type that owns a contract area; every other surface
either derives from it or is checked against it. When a surface disagrees
with its owner, the owner is right — fix the surface, not this table.

| Contract area | Canonical owner | Checked by |
|---|---|---|
| Agent/session/run lifecycle states | `crates/graphe/src/types.rs` (`SessionStatus`, `SessionType`) | `crates/graphe/tests/session_lifecycle.rs` |
| Tool capability metadata and approval states | `crates/organon` (`ToolRegistry`) | `crates/organon/tests/builtin_contracts.rs` |
| Memory/context provenance and selection state | `crates/mneme/src/run_context.rs` | — (aletheia#4540 tracks a versioned `RunRecord` unifying this) |
| Provider/model configuration and resolved runtime settings | `crates/taxis/src/config` (`AletheiaConfig`) | `configuration-doc-check.yml` (`docs/CONFIGURATION.md` generated/checked against the schema) |
| Streaming/SSE event names and payload semantics | `crates/pylon/src/handlers/streaming.rs` (server) / `crates/theatron/skene/src/api/types/mod.rs` (`SseEvent`, the typed client-side contract both proskenion and koilon consume) | `crates/integration-tests/tests/proskenion_contract.rs` (desktop); `crates/theatron/koilon/src/mapping/mod.rs::every_sse_event_variant_is_handled` (TUI) |
| Error/failure taxonomy and recovery guidance | `crates/eidos/src/failure.rs` (`FailureCategory`, `Recoverability`, `NextAction`) | `crates/pylon/src/error.rs::failure_taxonomy_covers_representative_categories`; wired into every pylon `ErrorBody` (aletheia#4545) |
| Metrics/cost/token envelopes | Each crate's own `src/metrics.rs` `register()` (LLM-provider family: `crates/hermeneus/src/metrics.rs`) | `metrics-doc-check.yml` (aletheia#4526) |
| Feature maturity/stability labels | `docs/MATURITY.md` | — (hand-maintained; aletheia#4537) |
| Route shapes | `crates/pylon/src/openapi.rs` (generated OpenAPI spec) | `crates/pylon/src/tests/route_contract.rs` (skene routes vs OpenAPI) |
| Crate dependency graph | `Cargo.toml` workspace members | `crate-index-check.yml` (`CRATE-INDEX.toml` generated/checked) |
| Crate/API surface reference | `_llm/L3-api-index/` (generated from source) | `llm-freshness.yml` |

## Adding a new runtime state, event, or tool field

1. Change it at the canonical owner listed above.
2. Update or add the "Checked by" test/gate for that row so a future change
   to the owner without updating dependents fails CI, not a user's session.
3. If no owner or check exists yet for the area you're touching, add both —
   a hand-maintained description with no check is exactly the drift this
   table exists to stop (aletheia#4544).
4. If the change affects `docs/CONFIGURATION.md`, `CRATE-INDEX.toml`, or a
   metrics name, the matching generator/check above already covers it —
   run it locally before committing (`--check` mode on each script).

## Known gaps

- **Memory/context provenance** has no single persisted schema yet — pieces
  exist (`ContextItem`, `ContextEvidenceRef`, `RunMemoryUpdate` in
  `crates/mneme/src/run_context.rs`) but no unifying `RunRecord` type
  composes them (aletheia#4540).
- **Feature maturity** is hand-maintained prose with no generator or
  freshness check (aletheia#4537).
- koilon has no wire-parsing code of its own — it re-exports
  `skene::api` verbatim (`crates/theatron/koilon/src/api/mod.rs`), so its
  contract test lives in-crate (`mapping::tests`) rather than as an external
  `tests/*.rs` integration test: `map_sse`/`map_event` are crate-private and
  an external test binary cannot reach them.
