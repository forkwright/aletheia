# L1 — Workspace Overview

## Crate list

| Crate | Path | Purpose |
|-------|------|---------|
| `koina` | `crates/koina` | Core types, errors, tracing, system abstractions shared by all crates |
| `taxis` | `crates/taxis` | Config cascade (TOML + env), path resolution, oikos directory structure |
| `eidos` | `crates/eidos` | Shared knowledge types: Fact, Entity, Relationship, EpistemicTier |
| `graphe` | `crates/graphe` | Session + message persistence (fjall LSM-tree) |
| `episteme` | `crates/episteme` | Knowledge pipeline: extraction, recall, consolidation, embeddings |
| `krites` | `crates/krites` | Embedded Datalog engine with HNSW vector search and graph algorithms |
| `mneme` | `crates/mneme` | Facade re-exporting eidos, graphe, episteme, krites for downstream crates |
| `hermeneus` | `crates/hermeneus` | Anthropic LLM client: streaming, retries, cost tracking, provider trait |
| `organon` | `crates/organon` | Tool registry + 49 built-in tool executors + sandbox (Landlock/seccomp) |
| `symbolon` | `crates/symbolon` | JWT auth, API keys, Argon2id passwords, RBAC |
| `melete` | `crates/melete` | Context distillation: LLM-driven conversation compression |
| `agora` | `crates/agora` | Channel registry + ChannelProvider trait + Signal implementation |
| `daemon` | `crates/daemon` | Per-nous background tasks: cron scheduling, prosoche, watchdog |
| `dianoia` | `crates/dianoia` | Multi-phase planning state machine with workspace persistence |
| `thesauros` | `crates/thesauros` | Domain pack loader: external knowledge, tools, config overlays |
| `nous` | `crates/nous` | Agent session pipeline: bootstrap, recall, execute, finalize (NousActor) |
| `pylon` | `crates/pylon` | Axum HTTP gateway: SSE streaming, auth middleware, rate limiting |
| `diaporeia` | `crates/diaporeia` | MCP server interface for external AI agents |
| `energeia` | `crates/energeia` | Dispatch orchestration: plan execution with budget and QA gating |
| `skene` | `crates/theatron/skene` | Shared API client, types, SSE parser for UIs |
| `koilon` | `crates/theatron/koilon` | Ratatui terminal dashboard |
| `theatron` | `crates/theatron` | Presentation umbrella re-exporting skene types |
| `proskenion` | `crates/theatron/proskenion` | Dioxus desktop app (excluded from workspace, requires GTK3) |
| `poiesis-core` | `crates/poiesis/core` | Format-agnostic document model: Document, Block, Renderer trait |
| `poiesis-text` | `crates/poiesis/text` | PDF + ODT rendering backends |
| `poiesis-sheet` | `crates/poiesis/sheet` | XLSX + ODS rendering backends |
| `poiesis-slides` | `crates/poiesis/slides` | PPTX rendering backend |
| `poiesis-lint` | `crates/poiesis/lint` | Prose-quality linting: banned words, citations, required sections |
| `poiesis-verify` | `crates/poiesis/verify` | Claim verification: arithmetic formula evaluation, cross-claim references |
| `dokimion` | `crates/eval` | Behavioral eval framework: HTTP scenario runner against live instances |
| `basanos` | `crates/basanos` | Planning and standards linter for kanon projects |
| `integration-tests` | `crates/integration-tests` | Cross-crate integration test suite |
| `aletheia` | `crates/aletheia` | Binary entrypoint: Clap CLI, service wiring |

## Layer grouping

**Foundations** (leaf nodes, no workspace deps):
`koina`, `eidos`, `dianoia`, `poiesis-core`

**Knowledge** (memory layer):
`graphe` → `episteme` → `krites` → `mneme` (facade)

**LLM integration**:
`hermeneus` (provider trait + Anthropic client), `melete` (distillation), `organon` (tools + sandbox)

**Nous / agent runtime**:
`taxis` (config), `symbolon` (auth), `daemon` (background tasks), `thesauros` (domain packs), `nous` (pipeline), `energeia` (dispatch)

**Gateway**:
`pylon` (HTTP), `diaporeia` (MCP), `agora` (channels)

**Operator / presentation**:
`skene`, `koilon`, `theatron`, `proskenion`, `poiesis-{text,sheet,slides}`

**Report rendering** (document generation):
`poiesis-core`, `poiesis-text`, `poiesis-sheet`, `poiesis-slides`, `poiesis-lint`, `poiesis-verify`

**Support** (not in application dep graph):
`dokimion`, `basanos`, `integration-tests`, `aletheia` (binary)

## Dependency direction

Imports flow strictly downward: foundations → knowledge → LLM integration → nous → gateway → binary. No reverse edges. Lower layers must not import from higher layers.

## Where to look for X

| Task | Crate | File |
|------|-------|------|
| Add HTTP endpoint | `pylon` | `src/handlers/` + register in `src/router.rs` |
| Add built-in tool | `organon` | `src/builtins/` + register in `register_all()` |
| Add CLI subcommand | `aletheia` | `src/commands/` + add to clap derive in `main.rs` |
| Add config section | `taxis` | `src/config.rs` → `AletheiaConfig` |
| Add MCP tool | `diaporeia` | `src/tools/mod.rs` |
| Add agent pipeline stage | `nous` | `src/pipeline/` |
| Add background task | `daemon` | `src/schedule.rs` → `BuiltinTask` |
| Add knowledge type | `eidos` | `src/knowledge.rs` |
| Add channel provider | `agora` | implement `ChannelProvider` trait in `src/` |
| Add domain pack | `thesauros` | `pack.toml` manifest, `PackManifest` |
| Add middleware | `pylon` | `src/middleware/` + `src/server.rs` |
| Add bootstrap file | `nous` | `src/bootstrap/` |
