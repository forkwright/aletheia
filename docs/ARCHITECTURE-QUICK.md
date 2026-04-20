# Aletheia Architecture Quick Reference

## Types & domain
- **koina** - core types, errors, tracing, system abstractions
- **eidos** - shared knowledge types
- **mneme** - facade re-exporting memory sub-crates

## Storage
- **graphe** - SQLite session/message store
- **krites** - embedded Datalog engine
- **episteme** - knowledge pipeline: extraction, recall, embeddings

## Runtime
- **hermeneus** - LLM client: streaming, retries, health tracking
- **organon** - tool registry, sandbox, 38 built-in tools
- **melete** - context distillation / summarization
- **thesauros** - domain pack loader
- **nous** - agent runtime: actor model, turn pipeline

## HTTP
- **symbolon** - auth: JWT, API keys, OAuth, RBAC
- **pylon** - Axum HTTP gateway: SSE, auth, rate limits
- **diaporeia** - MCP server for external AI agents

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

See [ARCHITECTURE.md](ARCHITECTURE.md) for full details.
