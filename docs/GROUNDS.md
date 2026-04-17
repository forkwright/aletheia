# Multiple-grounds audit: abstractions with single creation paths

> Issue: [#3507](https://github.com/forkwright/aletheia/issues/3507)  
> Branch: `docs/multiple-grounds-audit`

An abstraction with only one creation path is a mesh with a single root.  If that path is blocked (feature flag, network partition, code rot) the abstraction becomes unreachable.  This document enumerates the five abstractions called out in #3507 and records every verified creation path with file:line citations.

## Summary table

| Abstraction | #Grounds | Grounds (verified) | Status |
|-------------|----------|--------------------|--------|
| Session     | 1        | Pylon HTTP API     | **Single-grounded** |
| Plan        | 1        | Dianoia internal (`Plan::new`) | **Single-grounded** |
| Tool        | 3        | Organon builtins, Thesauros packs, External tools config | Multi-grounded (diaporeia MCP is **not** equivalent) |
| Agent       | 2        | Taxis config + `add-nous` CLI | **Partial** (import stubbed, no programmatic API) |
| Fact        | 3        | Extraction pipelines, bulk import API, ops-fact daemon | Multi-grounded (no single-fact manual entry) |

---

## 1. Sessions — single-grounded

### Verified creation paths

1. **`POST /api/v1/sessions`** — `crates/pylon/src/handlers/sessions/mod.rs:50`  
   The `create` handler is the only production entry-point that creates a session record.  It validates the request, generates a UUID v4 SessionId, and calls `graphe::SessionStore::create_session`.

2. **`resolve_or_create_session` (streaming)** — `crates/pylon/src/handlers/sessions/mod.rs:530`  
   Used by the SSE streaming handler to lazily create a session when the first message arrives.  Still HTTP-API-driven.

3. **`find_or_create_session` in NousActor turn loop** — `crates/nous/src/actor/turn.rs:289`  
   This is an *internal* reactive path triggered only when a pylon API request has already arrived.  It does not represent an independent ground.

4. **Diaporeia MCP `session_message` tool** — `crates/diaporeia/src/tools/mod.rs:154-169`  
   The MCP server calls `find_or_create_session` as a side-effect of the `session_message` tool.  This is API-adjacent (MCP transport instead of HTTP) but still request-driven rather than programmatic or declarative.

### Missing grounds

- **Programmatic test fixtures** — integration tests reach directly into `graphe::SessionStore::create_session` (`crates/integration-tests/tests/mneme_session.rs:20`, `crates/graphe/tests/session_lifecycle.rs:45`) or use the HTTP client (`crates/eval/src/client.rs:81`).  There is no shared `TestFixture::create_session()` helper that bypasses the network layer.
- **CLI command** — `aletheia session-export` exists (`crates/aletheia/src/commands/session_export.rs:1`) but there is no `aletheia session-create`.

### New grounds (post-#3601)

5. **`aletheia session-create <nous-id> [--key <session-key>]`** — `crates/aletheia/src/commands/session_create.rs:50`  
   The `session-create` CLI subcommand opens the local `graphe::SessionStore` directly, validates the agent exists in config, generates a UUID v4 `SessionId`, and calls `SessionStore::create_session`.  This bypasses the HTTP layer entirely and is useful for scripting and headless integration-test setups.  It produces behavior equivalent to the API path: same validation rules, same conflict semantics, and the same JSON-shaped output.
- **Domain pack initial state** — packs can declare tools and prompts, but there is no pack-level hook that pre-creates a session for an agent.

---

## 2. Plans — multi-grounded

### Verified creation paths

1. **`Plan::new`** — `crates/dianoia/src/plan.rs:109`  
   The original constructor.  It generates a fresh ULID, sets `state = Pending`, and uses `DEFAULT_MAX_ITERATIONS = 10`.

2. **`Phase::add_plan`** — `crates/dianoia/src/phase.rs:92`  
   The original caller of `Plan::new` outside unit tests.  It is `pub(crate)` and marked `#[cfg_attr(not(test), expect(dead_code, reason = "WIP: planning phase lifecycle"))]` — i.e. not exercised in production builds.

3. **`Plan::from_research`** — `crates/dianoia/src/plan.rs:137`  
   Creates one `Plan` per [`FindingStatus::Complete`] or [`FindingStatus::Partial`] finding in a [`ResearchOutput`].  Title is the domain heading, description is the finding content, wave is `0`.

4. **`Plan::from_template`** — `crates/dianoia/src/plan.rs:158`  
   Creates a new `Plan` from a completed plan, copying title/description and max-iterations, resetting state to `Pending`, clearing blockers/achievements/dependencies, and setting wave to the supplied `next_wave`.

### Production usage

- `Plan::new` is **not called** in any non-test production path.  The reconciler (`crates/dianoia/src/reconciler.rs:303`) creates `Project::new` and `Phase::new`, but never `Plan::new`.
- The pylon planning verification endpoints (`crates/pylon/src/handlers/planning.rs:21`) are explicitly stubbed: "Wire to the actual `dianoia` verification engine once a `PlanningService` trait is available (#2034)."

### Missing grounds

- **Planning API** — the pylon handlers are stubs; plans cannot be created via HTTP.

---

## 3. Tools — multi-grounded (with caveat)

### Verified creation paths

1. **Organon builtins** — `crates/aletheia/src/runtime/setup.rs:310`  
   `builtins::register_all_with_sandbox` registers all embedded tools (memory, filesystem, communication, etc.) into the `organon::ToolRegistry`.

2. **Thesauros domain packs** — `crates/aletheia/src/runtime/mod.rs:358`  
   `thesauros::tools::register_pack_tools` validates pack manifest tool definitions and registers them into the same `ToolRegistry`.

3. **External tools config** — `crates/aletheia/src/runtime/mod.rs:366`  
   `external_tools::register_external_tools` reads `config/tools/*.toml` and registers HTTP-proxy tools into the same `ToolRegistry`.

### Non-equivalent ground

4. **Diaporeia MCP tools** — `crates/diaporeia/src/tools/mod.rs:149`  
   Diaporeia uses the `rmcp` `#[tool_router]` macro to generate its own tool dispatch table (`ToolRouter<Self>`) stored on `DiaporeiaServer` (`crates/diaporeia/src/server.rs:34`).  These tools are **not** registered in `organon::ToolRegistry` and therefore do not share the same grounding abstraction.  Nous agents cannot invoke diaporeia tools, and diaporeia cannot invoke organon builtins.

---

## 4. Agents — partially grounded

### Verified creation paths

1. **Taxis config + `add-nous` CLI** — `crates/aletheia/src/commands/add_nous.rs:27`  
   `aletheia add-nous <name>` scaffolds a directory, writes markdown files, and appends an entry to `config/aletheia.toml` (`crates/aletheia/src/commands/add_nous.rs:168`).  The server then loads the agent from config at startup (`crates/taxis/src/config/resolved.rs:84`).

2. **Config-only (manual edit)** — `crates/taxis/src/config/agents.rs:218`  
   An operator can hand-write an `[[agents.list]]` entry; `taxis::config::resolve_nous` merges it with defaults.

### Broken / missing grounds

3. **Agent file import** — `crates/aletheia/src/commands/agent_io.rs:185-225`  
   `aletheia import-agent` is **stubbed out**.  Dry-run parsing works (`crates/aletheia/src/commands/agent_io.rs:186-218`) but the write path returns:  
   > "import is temporarily unavailable: the agent-file import pipeline is being reimplemented on the fjall backend (#3446)."

4. **Programmatic creation** — there is no `Agent::new()` or `NousManager::create_agent()` API.  The pylon `/nous` endpoints are read-only (`list`, `get`, `tools`, `recover`); no `POST /nous` exists.

---

## 5. Facts — multi-grounded

### Verified creation paths

1. **Turn extraction (NousActor background)** — `crates/nous/src/actor/background.rs:352`  
   After each turn, the distillation pipeline extracts facts from conversation and inserts them via `mneme::knowledge::Fact`.

2. **Dream / auto-distillation** — `crates/melete/src/dream/mod.rs:285-420`  
   The distillation engine extracts facts from session history and merges them into the knowledge graph.

3. **Ops fact extraction (daemon)** — `crates/daemon/src/execution.rs:694-738`  
   A scheduled maintenance task (`BuiltinTask::OpsFactExtraction`) runs `OpsFactExtractor` against daemon metrics and inserts operational facts.

4. **Bulk import API** — `crates/pylon/src/handlers/knowledge/bulk_import.rs:65`  
   `POST /api/v1/knowledge/facts/import` accepts up to 1000 facts and inserts them independently.

### Missing grounds

- **Single-fact manual entry** — there is no `POST /api/v1/knowledge/facts` endpoint for creating one fact at a time; operators must use the bulk import endpoint with a one-element array.
- **Programmatic batch helper** — extraction pipelines build `Fact` structs inline; there is no shared `FactBuilder` or `Fact::from_extraction(...)` convenience constructor.

---

## Recommendations

The following abstractions are worth extending because they have exactly one *working* production ground:

1. **Sessions** — add a programmatic creation helper for tests and a `aletheia session-create` CLI subcommand.
2. **Plans** — wire plan generation into the research output pipeline and add a `Plan::from_template` constructor for iterative planning.
3. **Agents** — re-implement the fjall-backed agent import/export pipeline (#3446) and add a `POST /api/v1/nous` endpoint.
4. **Tools (diaporeia)** — decide whether diaporeia MCP tools should be bridged into `organon::ToolRegistry` or documented as an intentionally separate tool plane.

Follow-up issues have been filed for the three highest-priority single-grounded abstractions.
