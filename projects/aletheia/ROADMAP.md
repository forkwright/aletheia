# Roadmap -- Aletheia

## Active

### Phase 12: Document generation
**Goal:** Operators can generate structured documents (spreadsheets, presentations, text) from agent outputs and knowledge graphs.

Success criteria:
- Poiesis crate produces valid ODS, ODP, and ODT files
- Document templates can reference knowledge graph entities
- Generation latency under 2s for 10-page documents

## Planned

### Phase 13: Desktop client
**Goal:** A native desktop application provides a graphical interface to all Aletheia capabilities.

Success criteria:
- Proskenion runs on Linux, macOS, and Windows
- Desktop parity with TUI for session management and memory browsing
- Binary size under 50 MB

## Completed

### Phase 01: Core foundations ✓
**Goal:** Establish the shared type system, error taxonomy, tracing, and identifier infrastructure that all downstream crates depend on.
**Completed:** 2025-01

### Phase 02: Memory system ✓
**Goal:** Conversations persist across sessions with SQLite-backed session storage and shared knowledge types.
**Completed:** 2025-02

### Phase 03: Knowledge engine ✓
**Goal:** The agent extracts facts, entities, and relationships from conversations and stores them in a queryable knowledge graph.
**Completed:** 2025-03

### Phase 04: Agent pipeline ✓
**Goal:** A complete turn-processing pipeline with bootstrap, recall, reasoning, and finalize stages.
**Completed:** 2025-04

### Phase 05: Tool system ✓
**Goal:** 40+ built-in tools covering filesystem, HTTP, web search, memory search, and agent coordination.
**Completed:** 2025-05

### Phase 06: HTTP gateway ✓
**Goal:** Full HTTP API with SSE streaming, rate limiting, field-level validation, and OpenAPI documentation.
**Completed:** 2025-06

### Phase 07: Auth and sessions ✓
**Goal:** JWT-based authentication, session management, and RBAC for multi-user instances.
**Completed:** 2025-07

### Phase 08: Channel system ✓
**Goal:** Agents can communicate over multiple channels including Signal messenger.
**Completed:** 2025-08

### Phase 09: Dispatch orchestration ✓
**Goal:** Background task scheduling, cron jobs, and pipeline dispatch stages for autonomous agent operation.
**Completed:** 2025-09

### Phase 10: Evaluation framework ✓
**Goal:** Behavioral evaluation system with scenario-based API testing against live instances.
**Completed:** 2025-10

### Phase 11: Terminal dashboard ✓
**Goal:** Rich TUI with markdown rendering, session management, and real-time streaming.
**Completed:** 2025-11
