# L1 - Workspace Overview

Aletheia is a single-binary Rust agent runtime with 44 workspace crates plus the excluded `proskenion` desktop shell. Imports flow from leaf/foundation crates upward into memory, tools, runtime, gateways, and finally the `aletheia` binary. Lower layers must not depend on higher layers; facade crates (`mneme`, `theatron`) exist to stabilize downstream imports, not to hide arbitrary logic.

## Crate list

| Crate | Path | Purpose |
|-------|------|---------|
| `aletheia` | `crates/aletheia` | Aletheia cognitive agent runtime. |
| `aletheia-classify` | `crates/aletheia-classify` | aletheia-classify crate in the Aletheia workspace. |
| `aletheia-lexica` | `crates/aletheia-lexica` | Static lexicon and data constants for Aletheia. |
| `aletheia-memory-mcp` | `crates/aletheia-memory-mcp` | Standalone stdio MCP server exposing Aletheia's memory and token-gated write tools to external agents. |
| `aletheia-routing` | `crates/aletheia-routing` | Shared routing trait and empirical success-rate storage for dispatch and interactive paths. |
| `diaporeia` | `crates/diaporeia` | MCP server interface - the passage through for external AI agents. |
| `dianoia` | `crates/dianoia` | Planning and project orchestration - multi-phase state machine with workspace persistence. |
| `eidos` | `crates/eidos` | Shared knowledge types for the Aletheia memory layer. |
| `energeia` | `crates/energeia` | Dispatch orchestration - actualization of plans into execution. |
| `episteme` | `crates/episteme` | Knowledge pipeline for Aletheia. |
| `agora` | `crates/agora` | Channel registry and provider implementations (Signal). |
| `oikonomos` | `crates/daemon` | Oikonomos (the steward) -- per-nous background task runner, cron scheduling, prosoche attention. |
| `dokimion` | `crates/eval` | Behavioral eval framework - scenario-based API testing against a live instance. |
| `hermeneus` | `crates/hermeneus` | LLM provider abstraction and Anthropic streaming client. |
| `integration-tests` | `crates/integration-tests` | integration-tests crate in the Aletheia workspace. |
| `graphe` | `crates/graphe` | Session persistence layer for Aletheia. |
| `koina` | `crates/koina` | Core types, errors, and tracing for Aletheia. |
| `krites` | `crates/krites` | Embedded Datalog engine with HNSW and graph support for Aletheia. |
| `mneme` | `crates/mneme` | Session store and memory engine for Aletheia. |
| `melete` | `crates/melete` | Context distillation engine - compresses conversation history. |
| `nous` | `crates/nous` | Agent session pipeline - bootstrap, routing, tool execution. |
| `organon` | `crates/organon` | Tool registry, definitions, and built-in tool executors. |
| `pylon` | `crates/pylon` | Axum HTTP gateway for Aletheia. |
| `symbolon` | `crates/symbolon` | Authentication and authorization for Aletheia. |
| `taxis` | `crates/taxis` | Configuration cascade and path resolution for Aletheia. |
| `thesauros` | `crates/thesauros` | Domain pack loader - external knowledge, tools, and config overlays. |
| `theatron` | `crates/theatron` | Thin facade re-exporting skene types for external consumers. |
| `skene` | `crates/theatron/skene` | Shared API client, types, SSE, and streaming infrastructure for Aletheia UIs. |
| `koilon` | `crates/theatron/koilon` | Terminal dashboard for the Aletheia distributed cognition system. |
| `poiesis-core` | `crates/poiesis/core` | Format-agnostic document model and Renderer trait for poiesis. |
| `poiesis-text` | `crates/poiesis/text` | PDF and ODT document rendering backends for poiesis. |
| `poiesis-sheet` | `crates/poiesis/sheet` | XLSX and ODS spreadsheet rendering backends for poiesis. |
| `poiesis-slides` | `crates/poiesis/slides` | PPTX presentation rendering backend for poiesis. |
| `poiesis-lint` | `crates/poiesis/lint` | Report prose linting: banned words, citation checks, structure checks. |
| `poiesis-scaffold` | `crates/poiesis/scaffold` | Project-template scaffolder for poiesis report projects. |
| `poiesis-typst` | `crates/poiesis/typst` | Typst-based PDF rendering backend for poiesis: embeddable compiler, JSON data injection, template assets. |
| `poiesis-verify` | `crates/poiesis/verify` | Report claim verification: arithmetic evaluation and source resolution. |
| `poiesis-intake` | `crates/poiesis/intake` | Parse Slack-style request text into a structured report scaffold. |
| `poiesis-doc` | `crates/poiesis/doc` | DOCX write and inspect backend for poiesis. |
| `poiesis-diff` | `crates/poiesis/diff` | Cell-level diff for XLSX and PPTX documents. |
| `poiesis-inspect` | `crates/poiesis/inspect` | Text extraction from PDF, XLSX, and PPTX documents. |
| `gnosis` | `crates/gnosis` | Machine-derived code-graph index for symbol-level cross-crate queries. |
| `aletheia-sessions-migrate` | `crates/aletheia-sessions-migrate` | One-shot SQLite v32 -> fjall sessions-store migrator for legacy aletheia 0.15.x instances. |
| `proskenion` | `crates/theatron/proskenion` | Dioxus desktop shell for Aletheia (excluded from the workspace build). |

## Layer grouping

**Foundations.** Core data, errors, classification, and static facts: `koina`, `eidos`, `dianoia`, `aletheia-lexica`.

**Storage and knowledge.** Sessions, Datalog, recall, ingestion, and memory facade: `graphe`, `krites`, `episteme`, `mneme`, `gnosis`.

**LLM, tools, and runtime.** Providers, tools, distillation, domain packs, and the agent actor pipeline: `hermeneus`, `organon`, `melete`, `thesauros`, `nous`.

**Auth, gateway, MCP, and channels.** HTTP, auth/RBAC, external MCP, Signal/channel routing: `symbolon`, `pylon`, `diaporeia`, `agora`, `aletheia-memory-mcp`.

**Dispatch, daemon, and routing.** Background maintenance, empirical routing, and dispatch orchestration: `oikonomos`, `aletheia-routing`, `energeia`.

**CLI and operators.** Binary wiring, migrations, evals, and integration canaries: `aletheia`, `aletheia-sessions-migrate`, `dokimion`, `integration-tests`.

**Poiesis document stack.** Report model, renderers, diff/inspect/intake/scaffold helpers: `poiesis-core`, `poiesis-text`, `poiesis-sheet`, `poiesis-slides`, `poiesis-lint`, `poiesis-verify`, `poiesis-typst`, `poiesis-intake`, `poiesis-doc`, `poiesis-diff`, `poiesis-inspect`, `poiesis-scaffold`.

**Presentation.** Shared UI client, TUI, facade, and excluded desktop shell: `skene`, `koilon`, `theatron`, `proskenion`.

## Dependency direction

`koina`/`eidos`/`dianoia` provide leaf types. `graphe`, `krites`, and `episteme` build the memory substrate and are re-exported through `mneme`. `hermeneus`, `organon`, `melete`, and `thesauros` support the `nous` actor pipeline. `pylon`, `diaporeia`, and `agora` expose the runtime over HTTP, MCP, and channels. `aletheia` wires configuration, stores, providers, actors, and gateways at the top.

## Where to look for X

| Task | Crate | File |
|------|-------|------|
| Add HTTP endpoint | `pylon` | `crates/pylon/src/handlers/ + router/openapi` |
| Add built-in tool | `organon` | `crates/organon/src/builtins/ + register_all()` |
| Add config field | `taxis` | `crates/taxis/src/config/behavior/ + registry metadata` |
| Add knowledge type | `eidos` | `crates/eidos/src/knowledge/` |
| Add Datalog rule | `krites` | `crates/krites` |
| Add pipeline stage | `nous` | `crates/nous/src/pipeline/` |
| Add maintenance task | `oikonomos` | `crates/daemon/src/maintenance/ + runner registration` |
| Add MCP tool | `diaporeia` | `crates/diaporeia/src/tools/` |
| Add CLI command | `aletheia` | `crates/aletheia/src/commands/` |
| Add report backend | `poiesis-*` | `crates/poiesis/<backend>/` |
