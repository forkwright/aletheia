# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

### Added

#### Engine
- Skill quality lifecycle: usage tracking, score decay, and deduplication (#740)
- Actor crash recovery with liveness detection and auto-restart (#739)
- Explicit forgetting with reason tracking (F.6) (#736)
- LLM-driven fact consolidation pipeline (#731)
- Instinct system for behavioral memory from tool usage (#729)
- Ecological succession and domain volatility tracking (#728)
- Graph algorithms wired into recall scoring pipeline (#725)
- Entity deduplication pipeline (#723)
- Conflict detection for fact insertion (#713)
- FSRS power-law decay replacing exponential recency model (#715)
- Context-dependent extraction refinement (#714)
- Knowledge maintenance task framework in daemon (#711)
- Skill auto-capture: heuristic filter, candidate tracker, LLM extraction (#692, #696)
- Skill export to Claude Code format (#695)
- Fjall storage and hnsw_rs search replacing RocksDB (#686)
- Skill storage and seeding CLI (#676)
- Bi-temporal queries and `datalog_query` tool (#654)
- LLM-powered query rewriting and multi-tier recall (#641)
- KnowledgeStore enabled in production binary (#535)
- Data lifecycle: retention policies, migrations, and backup (#471)

#### Pipeline
- Task backoff, hung-task detection, cron catch-up, and system health checks (#738)
- Landlock + seccomp sandbox for tool execution (#664)
- RAII cleanup guards for processes, temp dirs, and storage (#677)
- Programmatic tool calling and code execution in hermeneus (#683)
- Native Anthropic server-side tools (#659)
- Message-level `cache_control` for prompt caching (#658)
- Anthropic API parity: caching, token counting, `tool_choice`, citations, batch (#511)
- Dynamic credential provider with OAuth refresh (#506)
- Anthropic server-side web search replacing Brave scraper (#534)
- Sub-agent spawn and dispatch tool executors (#528)
- `enable_tool`, `web_search`, `web_fetch` tool executors (#530)
- `view_file` multimodal tool for images, PDFs, and rich output (#505)
- Memory, communication, and workspace navigation tool executors (#525, #524, #523)
- Planning tool executors for dianoia (#529)
- Behavioral eval framework (dokimion) for pre-cutover validation (#504)
- Structured observability: pipeline spans, tool metrics, provider telemetry (#476)

#### TUI
- Memory inspector panel: browse, search, and manage knowledge (#689)
- Multi-session tabs with quick-switch keybindings (#687)
- OSC 8 clickable hyperlinks in chat messages (#688)
- Operations pane: thinking, tool calls, and diffs (#678)
- Virtual scrolling for O(viewport) rendering (#685)
- Diff viewer with unified, side-by-side, and word-diff modes (#682)
- Stack-based navigation with breadcrumbs (#672)
- Conversation management: list, switch, create, rename, archive (#666)
- Row-level message selection with context-sensitive actions (#662, #512)
- Command palette extensions: dynamic agents, shortcuts, live commands (#510)
- Context-aware help system (#509)
- Live filtering with `/` mode (#513)
- Config settings overlay with read/write API (#514)
- `aletheia tui` subcommand wired into main binary (#519)

#### Deployment
- Graceful shutdown with `CancellationToken` propagation (#680)
- Resilience foundation: health tracking, user error budgets, reconnect (#492)
- Runtime security: TLS, CORS, rate limits, headers, redaction, CSRF (#491)
- OpenAPI spec with versioned routes and typed error shape (#493)
- Prometheus metrics and `aletheia status` CLI (#494)
- Agent export/import portability (autarkeia) (#507)
- Automatic distillation for session lifecycle management (#527)
- Webchat compatibility endpoints for Svelte UI (#526)
- Security hardening v1: gitleaks, CODEOWNERS, commit signing (#470)

#### Testing
- Cross-crate integration test suite for cutover validation (#737)
- Baseline tests for all 18 graph algorithms and KCore (#709)
- Complete markdown renderer test coverage (#673)
- Phase B test hardening and instrumentation for mneme (#665)
- Builtin tool executor tests for organon (#661)
- mneme-engine and SQLite feature coexistence tests (#681)

### Changed

#### Engine
- `BoxErr` replaced with typed snafu enums in all mneme modules: runtime (#732), query (#726), data (#708), parse (#712), storage (#710)
- Phase C type safety and validation pass (#684)
- Phase E dependency upgrades and graph absorption in mneme (#652)
- `LlmProvider` and `AnthropicProvider` migrated to native async (#724)

#### TUI
- `api.rs` decomposed into client, types, and error modules (#702)
- Theme refactored to struct-of-structs for future theme switching (#704)
- Migrated from `anyhow` to `snafu` error handling (#671)
- Performance and code quality sweep (#653)

#### Deployment
- TLS consolidated to rustls-only; `openssl-sys` removed (#719)
- `reqwest` upgraded 0.12 → 0.13 with ring crypto provider (#721)
- Crate reorganization: oikonomos absorbed, bench removed (#490)
- Dependency hygiene: hf-hub defaults, chrono scope, deny.toml hardening (#720)

### Fixed

#### Engine
- HNSW bounded cache, panic elimination, and test coverage (#741)
- Eliminated `unwrap()` from mneme query, data, and native modules (#730, #722, #716)
- Audited all unsafe blocks with `SAFETY` documentation (#703)
- `await_holding_lock` promoted to deny in async lints (#690)
- Workspace path validation on startup (#669)
- Phase A stabilization: mneme query correctness (#649)
- Silent `unwrap_or` replaced with proper error propagation (#650)
- Mutex `.expect()` replaced with snafu error propagation (#647)

#### Pipeline
- OAuth tokens use Bearer header and beta flag (#518)
- Cutover follow-ups: signal receive, port binding, example config (#515)
- Bootstrap workspace file alignment with runtime validation (#533)
- Turn wall-clock timeout resolved from config (#480)
- Session store wired into NousManager (#469)
- Plans-db migration path and call-site bug (#487)
- CI format, clippy 1.94 compat, and test dependencies (#551)

#### TUI
- API endpoint paths corrected to `/api/v1/` prefix (#521, #520)
- Pre-cutover quality pass (#733)

### Removed
- TypeScript-era cruft and absorbed vendor sources (−142K lines) (#536)
- `openssl-sys` dependency; TLS unified under rustls (#719)

---


## [0.13.0] - 2026-03-19

### Added
- Backup cron script and standards-sync workflow (#1750)

### Changed
- Code quality: constants, re-exports, error types, `#[must_use]`, and doc cleanup (#1759)
- libc removed; all syscalls use rustix or inline asm (#1752)
- 43 files exceeding the 800-line limit split into focused submodules (#1681)

### Fixed
- Graceful shutdown, OOM handling, disk pressure, embedding errors, and streaming (#1758)
- Sandbox exec, SSRF protection, session IDs, paths, and config hardening (#1754)
- Eight init and CLI issues (#1757)
- Confidence update, hard session delete, and credential encryption (#1753)
- Eight deploy and operations script issues (#1746)
- Invalid OAuth beta header causing 400 errors from Anthropic API (#1744)
- Unsafe indexing replaced with `.get()` and justified expects in theatron-tui (#1693)
- `as_conversions`, `indexing_slicing`, and `string_slice` lint violations (#1682)
- Three runtime behavior bugs (#1679)

### Changed
- Datalog builders, deploy download, and standards-sync CI refactored (#1680)
- Remaining refactors: must_use, snafu location, test helpers (#1684)
- Freeform inline comments tagged or deleted across top 10 crates (#1683)
- Import ordering fixed; `println!` calls audited (#1685)

---


## [0.12.0] - 2026-03-16

### Added
- Structured JSON file logging with daily rotation
- MCP server rate limiting
- OAuth auto-refresh from Claude Code credentials
- Fuzz testing infrastructure (3 targets, 60+ seeds)
- Default agent tool permissions expanded
- Session display_name field
- Prebuilt binary releases in CI
- Theatron desktop scaffold (Dioxus 0.7)

### Fixed
- Landlock exec Permission Denied on kernel 6.18 (ABI v7)
- Knowledge facts API returning empty despite extraction
- embed-candle restored to default features
- CSRF and rate limit responses now include request_id
- JWT default signing key validated at startup
- Session limit parameter enforced
- Knowledge sort/order parameter validation
- Haiku 4.5 pricing configuration
- CrossNousRouter pending_replies leak
- 8 TUI bugs (streaming, scrolling, contrast, :recall, stale indicator)

### Changed
- Adapter traits use typed error enums (no more Result<T, String>)
- Hero functions decomposed in nous, pylon, organon
- Distillation trigger thresholds extracted to named constants
- Em dashes removed from inline comments
- 40+ commented-out debug prints removed
- Pylon isolated error response tests added
- Zero-coverage crates now have tests (agora, dianoia, diaporeia, thesauros)

## [1.3.0]: memory system audit & overhaul

### Added

#### Engine
- Memory extraction pipeline with contradiction detection
- Reinforcement loop, noise filter, and domain re-ranking
- Emergency distillation, memory health monitoring, and audit CLI
- RELATES_TO eliminated from controlled vocabulary; density validated across 1,194 edges
- Voyage-4 embeddings migration with UI redesign
- Memory evolution, deliberation, and discovery engine
- PII detection integrated into memory pipeline
- Working state, release workflow, and error boundaries in mneme

#### Pipeline
- Dianoia v1: persistent multi-phase planning runtime
- Dianoia v2: file-backed state and context packet
- Dianoia: enhanced execution engine and planning UI
- Dianoia: collaborative API, task system, and planning workspace
- Sub-agent infrastructure with session continuity
- Parallel tool dispatch with convergence
- Message queue, sub-agent spawn, and session auth
- TTS pipeline, event bus, and skill learning
- Hot-reload, hook system, and agent export
- Slash commands, thinking mode, and stop button
- Unified thread model and data privacy module
- MCP server, routing, planning tools, and CI/CD
- Search, embeddings, metrics, and tool policy

#### TUI
- TUI dashboard MVP (Phases 1–3)
- Svelte 5 web UI with streaming backend
- Graph visualization, file explorer, and pipeline decomposition
- Syntax highlighting, sidebar, slash commands, and file uploads
- Gold theme, design system, accessibility, and onboarding

#### Deployment
- JWT, RBAC, sessions, audit log, retention, and TLS
- Zero-config onboarding and hermeneus OAuth credential pill
- Research stack, service watchdog, and workspace tooling
- Complete test suite with >80% coverage

### Fixed
- Path traversal, RCE, auth bypass, and credential exposure
- SSE reliability and ChatView import errors
- Runtime hardening for plugins, models, and error propagation
- Hermeneus retry resilience, model fallback, and taxis state migration

### Changed
- UI overhauled: light theme, horizontal agent bar, debounce
- CSS design tokens and InputBar decomposition

---

## [1.2.0]: onboarding & mac support

### Added

#### Deployment
- macOS launchd boot persistence: `aletheia enable` / `aletheia disable`
- `aletheia doctor`: async connectivity, dependency, and boot persistence checks
- `aletheia init`: API key auto-detection, profile step, and start banner
- Bash 3.2 portability with parallel arrays replacing `declare -A`
- Cross-platform `setup.sh` with Homebrew guidance and boot persistence prompt
- macOS `nc -z` port check in setup script

### Fixed
- SSE heartbeat and 10-minute timeout fix for sub-agent stream timeouts
- Docker Compose portable volume definitions

### Changed
- DEPLOYMENT.md and QUICKSTART.md updated for macOS and Linux accuracy

---

[Unreleased]: https://github.com/forkwright/aletheia/compare/v0.13.0...HEAD
[0.13.0]: https://github.com/forkwright/aletheia/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/forkwright/aletheia/releases/tag/v0.12.0
