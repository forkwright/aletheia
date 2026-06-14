# Aletheia Architecture Quick Reference

## Types & domain
- **koina** - core types, errors, tracing, system abstractions
- **eidos** - shared knowledge types
- **mneme** - facade re-exporting memory sub-crates

## Storage
- **graphe** - fjall session/message store
- **krites** - embedded Datalog engine
- **episteme** - knowledge pipeline: extraction, recall, embeddings

## Runtime
- **hermeneus** - LLM client: streaming, retries, health tracking
- **organon** - tool registry, sandbox, HMAC receipts, built-in tools (see ARCHITECTURE.md for count)
- **melete** - context distillation / summarization
- **thesauros** - domain pack loader
- **nous** - agent runtime: actor model, working memory, turn pipeline, per-stage timeouts

## HTTP
- **symbolon** - auth: JWT, API keys, OAuth, admin facade, RBAC
- **pylon** - Axum HTTP gateway: SSE, auth, rate limits, meta-insights
- **diaporeia** - MCP server and external tool-plane bridge

## CLI & desktop
- **aletheia** - binary: CLI, server startup, wiring
- **theatron** - UI umbrella: skene, koilon (TUI), proskenion (desktop)
- **dianoia** - planning: multi-phase state machine
- **agora** - channel registry: Signal, etc.

## Ops & misc
- **taxis** - config cascade: TOML, oikos, hot-reload
- **daemon** - background tasks: cron, maintenance, watchdog
- **eval** - behavioral eval: scenario-based API testing
- **energeia** - dispatch orchestration
- **poiesis** - document rendering: PDF, ODT, XLSX, ODS, PPTX
- **gnosis** - code graph indexing: SHA-256, module path, re-export edges

See [ARCHITECTURE.md](ARCHITECTURE.md) for full details.
