# Model Capability Audit: Native vs Built

> Research document — March 2026
> Scope: What Aletheia builds vs what Claude models and Claude Code provide natively

---

## 1. Executive Summary

Aletheia is a 42K-line Rust agent runtime with 17 crates. After auditing every crate against current Claude API and Claude Code capabilities (as of Opus 4.6 / Sonnet 4.6, March 2026), the honest assessment:

**Keep and invest** (genuinely unique, no native equivalent):
- Persistent identity system (SOUL.md, relationship context, personality continuity)
- Bi-temporal knowledge graph with FSRS-based recall scoring (mneme)
- Cross-agent topology with inter-agent messaging (nous + agora)
- Domain pack injection system (thesauros)
- Attention-based heartbeat with proactive triggers (prosoche/daemon)
- Channel integration layer (agora — Signal, future Slack/Telegram)
- Encrypted-at-rest session storage with XChaCha20Poly1305

**Replace with native** (native is equal or better, maintenance cost not justified):
- Custom file tools (read/write/edit/grep/find) — CC native tools are superior
- Custom exec tool — CC native Bash is more capable
- Custom web search tool — Anthropic's `web_search_20250305` is server-side, higher quality
- Spawn service for sub-agents — CC native subagents + agent teams exceed this
- Tool whitelisting logic — CC `allowedTools` + hooks cover this
- Basic context assembly — CLAUDE.md cascade + skills system handles this

**Hybrid approach recommended** (use native for transport, custom for state):
- Context distillation (melete) — native compaction exists but lacks structured section preservation
- Planning orchestration (dianoia) — extended thinking handles reasoning, but multi-session project state needs custom persistence

**Not leveraging** (native capabilities we should use):
- Memory tool API (`memory_20250818`) — client-side persistent memory, directly comparable to our memory file system
- Context editing / compaction — server-side context management
- Structured output with guaranteed JSON schema adherence
- Prompt caching (5-min and 1-hour tiers) — would cut costs for repeated system prompts
- 1M token context window (beta on Opus 4.6)
- Agent Skills open standard — portable skill definitions across platforms
- Programmatic tool calling — server-side tool orchestration

---

## 2. Capability Matrix

| Capability | Claude API Native | Claude Code Native | Aletheia Built | Verdict |
|---|---|---|---|---|
| **File read/write/edit** | — | Read, Write, Edit tools (20 built-in tools) | `organon::filesystem` | **Replace** — CC tools are battle-tested, permission-aware |
| **Shell execution** | — | Bash tool with sandboxing | `organon::exec` | **Replace** — CC Bash has security sandboxing, hooks |
| **Web search** | `web_search_20250305` (server-side) | WebSearch, WebFetch | `organon::research` | **Replace** — native is server-side, no proxy needed |
| **Sub-agent spawning** | — | Subagents (up to 7 parallel), Agent Teams | `nous::SpawnService` | **Replace** — CC subagents have full tool access, isolation |
| **Tool registry** | Native tool use, parallel calling | 20 built-in + MCP tools | `organon::ToolRegistry` | **Keep for custom tools** — native handles standard tools, we handle domain-specific |
| **Context assembly** | System prompt, tool definitions | CLAUDE.md cascade, skills, hooks | `nous::bootstrap` | **Hybrid** — use CLAUDE.md for static, keep dynamic injection |
| **Tool whitelisting** | `tool_choice`, `disable_parallel_tool_use` | `allowedTools` + PreToolUse hooks | Custom logic | **Replace** — CC hooks are more flexible |
| **Session persistence** | Memory tool (`memory_20250818`) | Session Memory (auto-background) | `mneme::SessionStore` (SQLite) | **Keep** — our store has encryption, retention policies, structured queries |
| **Context distillation** | Compaction (server-side summarization) | `/compact` (instant via Session Memory) | `melete::DistillEngine` | **Hybrid** — native compaction for transport, keep melete for structured sections |
| **Knowledge graph** | — | — | `mneme::knowledge` (CozoDB Datalog + HNSW) | **Keep** — no native equivalent exists |
| **Recall scoring** | — | — | `mneme::recall` (6-factor: recency, access, FSRS, semantic, entity, temporal) | **Keep** — unique to Aletheia |
| **Fact extraction** | Citations API | — | `mneme::extract` (LLM-driven) | **Keep** — deeper than citations, feeds knowledge graph |
| **Extended thinking** | Adaptive thinking (Opus 4.6, Sonnet 4.6) | Built-in | `dianoia::Planning` | **Hybrid** — use native thinking for reasoning, keep dianoia for project state machine |
| **Structured output** | `output_config.format` (guaranteed schema) | — | Custom extraction prompts | **Adopt native** — guaranteed schema compliance |
| **Persistent identity** | — | — | SOUL.md, IDENTITY.md, relationship context | **Keep** — no native equivalent |
| **Cross-agent messaging** | — | Agent Teams (experimental) | `nous::CrossNousRouter` | **Keep for now** — agent teams are experimental, our routing is production-grade |
| **Channel integration** | — | — | `agora` (Signal via signal-cli) | **Keep** — domain-specific integration |
| **Background tasks** | — | — | `daemon/oikonomos` (cron, prosoche) | **Keep** — no native equivalent for scheduled attention |
| **Domain packs** | — | Skills (.claude/skills/) | `thesauros` (pack.yaml + tools + context) | **Hybrid** — skills handle instructions, keep packs for tool + context bundles |
| **Auth/RBAC** | — | — | `symbolon` (JWT, API keys, Argon2id) | **Keep** — deployment-specific |
| **HTTP gateway** | — | — | `pylon` (Axum + SSE) | **Keep** — the API surface |
| **TUI** | — | Built-in terminal UI | `tui` (ratatui) | **Keep** — Aletheia-specific monitoring dashboard |
| **Prompt caching** | 5-min and 1-hour cache tiers (0.1x read cost) | Automatic | Not used | **Adopt** — significant cost savings for repeated system prompts |
| **Memory tool** | `memory_20250818` (client-side CRUD) | MEMORY.md auto-managed | Workspace files (MEMORY.md, etc.) | **Evaluate** — API memory tool is structurally similar to our workspace file approach |
| **1M context window** | Beta on Opus 4.6 | 200K default | 200K assumed | **Evaluate** — could reduce distillation frequency |

---

## 3. Strategic Capabilities (Genuinely Unique)

### 3.1 Persistent Identity System

**What it is:** Each nous has SOUL.md (character, principles), IDENTITY.md (name, emoji), GOALS.md (active objectives), MEMORY.md (persistent knowledge), and CONTEXT.md (session state). These files persist across all sessions and define the agent's personality, relationship to the operator, and accumulated knowledge.

**Why it's unique:** No mainstream AI product maintains persistent identity across sessions with this granularity. Claude's memory tool stores facts but not personality, relationship dynamics, or goal hierarchies. ChatGPT's memory is flat key-value recall. Aletheia's identity system creates agents that know who they are, who they serve, and what they're working toward — across weeks and months.

**Competitive landscape:** Zep/Graphiti offers temporal knowledge graphs but not identity persistence. Character.ai has personality but no knowledge graphs. No product combines both.

**Verdict:** Core differentiator. Keep and invest.

### 3.2 Bi-Temporal Knowledge Graph with FSRS Recall

**What it is:** CozoDB Datalog engine with HNSW vector indexing, storing facts, entities, and relationships with four timestamps (created, expired, valid_from, valid_until). Recall scoring uses 6 factors: recency, access count, FSRS stability (spaced repetition), semantic similarity, entity relevance, temporal alignment.

**Why it's unique:** No Claude API feature provides anything comparable. The closest competitor is Zep's Graphiti (also bi-temporal, also knowledge graph), but Aletheia's recall scoring with FSRS decay is novel — knowledge that isn't accessed fades, simulating human memory dynamics. The controlled vocabulary with semantic invariants (RELATES_TO banned, unknown types pass through) ensures graph quality.

**Competitive landscape:**
- Zep/Graphiti: bi-temporal knowledge graph, similar architecture, but SaaS-only
- Mem0: knowledge layer for agents, but simpler (no graph algorithms, no FSRS)
- AWS AgentCore: long-term memory, but enterprise-focused, no graph
- None combine FSRS-based decay with graph algorithms (PageRank, Louvain clustering, shortest path)

**Verdict:** Core differentiator. Unique combination of techniques.

### 3.3 Cross-Agent Topology

**What it is:** `CrossNousRouter` enables fire-and-forget and request-response messaging between agents. Each nous is a Tokio actor with its own session state. `NousManager` handles lifecycle (spawn, address, shutdown). Multiple agents can coordinate on shared tasks.

**Why it's unique:** Claude Code's agent teams (experimental, February 2026) allow a "team lead" to coordinate "teammates," but teammates can't message each other directly without the lead as intermediary. Aletheia's topology is peer-to-peer — any nous can message any other nous directly, with delivery audit trails.

**Competitive landscape:**
- Claude Code Agent Teams: lead-spoke topology, experimental
- OpenAI Swarm: routing-based, single-threaded
- CrewAI/AutoGen: Python frameworks with delegation patterns
- None offer persistent peer-to-peer agent messaging with delivery guarantees in a compiled runtime

**Verdict:** Architectural advantage. Keep.

### 3.4 Domain Pack System (thesauros)

**What it is:** Portable directories containing `pack.yaml` manifest, tool definitions, context files, and config overlays. A pack can inject specialized knowledge, tools, and configuration into any nous without recompilation.

**Why it's unique:** Claude Code's skills system handles instruction sets (SKILL.md with metadata), but skills are text-only — they don't bundle tool definitions or config overlays. Aletheia's packs are richer: tools + knowledge + config in a single deployable unit.

**Competitive landscape:** Anthropic's Agent Skills open standard (December 2025) is the closest parallel — portable, cross-platform skill definitions. But skills are instructional while packs are functional (they include executable tool definitions and data).

**Verdict:** Keep, but evaluate convergence with Agent Skills standard for the instructional layer.

### 3.5 Prosoche (Directed Attention)

**What it is:** Periodic attention checks injected into the message loop via `daemon/oikonomos`. Categories: Calendar, Task, SystemHealth, Custom. Urgency levels: Low, Medium, High, Critical. The agent proactively monitors external state and can initiate actions without being prompted.

**Why it's unique:** No AI agent product has a built-in attention/heartbeat system that proactively triggers agent behavior based on scheduled checks. Claude Code agents are reactive — they respond to prompts. Aletheia agents can be proactive.

**Verdict:** Unique capability. Keep and invest.

---

## 4. Redundancy Candidates (Replace with Native)

### 4.1 Custom File Tools → CC Native Tools

**What we built:** `organon::filesystem` — `files_read`, `files_write`, `files_list`
**What native provides:** Claude Code has Read, Write, Edit, Glob, Grep tools — 20 built-in tools total, battle-tested across millions of sessions, with permission-aware sandboxing and hook integration.

**Assessment:** Our file tools are a strict subset of CC's capabilities. CC tools have better error handling, permission models, and user experience. Maintaining our own adds no value when running under CC.

**Recommendation:** Remove. When Aletheia runs standalone (not under CC), use the memory tool API or direct filesystem access via taxis/oikos.

### 4.2 Custom Exec Tool → CC Native Bash

**What we built:** `organon::exec` — shell command execution
**What native provides:** CC Bash tool with sandbox mode, timeout control, background execution, hook integration (PreToolUse/PostToolUse validation).

**Assessment:** CC's Bash tool has security sandboxing (seccomp-like restrictions), configurable timeouts, and permission prompts. Our exec tool would need to replicate all of this.

**Recommendation:** Remove for CC-hosted execution. Keep seccomp sandboxing (extrasafe) for standalone mode.

### 4.3 Custom Web Search → Anthropic Server-Side Search

**What we built:** `organon::research` — external API lookups
**What native provides:** `web_search_20250305` — server-side web search, no API key needed, integrated into the model's reasoning. Opus 4.6/Sonnet 4.6 can dynamically filter results.

**Assessment:** Server-side search is fundamentally better — no proxy, no API key management, integrated into the model's extended thinking. Our implementation would need to maintain search API keys and handle rate limiting.

**Recommendation:** Replace entirely. Use native web search tool.

### 4.4 Spawn Service → CC Subagents + Agent Teams

**What we built:** `nous::SpawnService` — ephemeral sub-agent spawning within the pipeline
**What native provides:** CC can spawn up to 7 parallel subagents, each with its own context window, tool access, and independent permissions. Agent Teams (experimental) add peer coordination.

**Assessment:** CC subagents are more capable than our spawn service — they have full tool access, filesystem isolation, and automatic context loading (CLAUDE.md, MCP, skills). Our spawn service operates within the nous pipeline, which is more constrained.

**Recommendation:** Replace for ephemeral tasks. Keep `NousManager` for persistent multi-agent orchestration (which CC subagents don't provide — they're ephemeral).

### 4.5 Tool Whitelisting → CC allowedTools + Hooks

**What we built:** Custom tool filtering logic in the pipeline
**What native provides:** `allowedTools` configuration, PreToolUse hooks for validation, per-subagent tool access control.

**Assessment:** CC's hook system is more flexible — you can validate tool inputs, block specific operations, run linters post-edit. Our whitelisting is simpler but less capable.

**Recommendation:** Replace when running under CC. Keep for standalone API mode.

### 4.6 Basic Context Assembly → CLAUDE.md Cascade

**What we built:** `nous::bootstrap` — system prompt construction from workspace files + domain packs
**What native provides:** CLAUDE.md cascade (global → project → directory rules), skills auto-loading, MCP server integration.

**Assessment:** Partial overlap. CC's cascade handles static configuration well. But our bootstrap does dynamic context injection (distillation state, memory flush, recall results, domain pack content) that CLAUDE.md can't replicate.

**Recommendation:** Hybrid — use CLAUDE.md for static agent configuration, keep bootstrap for dynamic runtime context (knowledge graph results, distillation state, etc.).

---

## 5. Unleveraged Native Features

### 5.1 Prompt Caching

**What it is:** Cache breakpoints in API requests. 5-minute cache (1.25x write, 0.1x read) and 1-hour cache (2x write, 0.1x read). Automatic prefix matching.

**Impact for Aletheia:** System prompts, workspace files, and tool definitions are largely static within a session. Caching them would reduce input costs by ~90% for repeated turns. With SOUL.md, GOALS.md, and tool definitions potentially consuming 10-20K tokens per turn, this is significant.

**Action:** Implement cache breakpoints in `hermeneus::AnthropicProvider` for system prompt and tool definition blocks.

### 5.2 Memory Tool API

**What it is:** Client-side CRUD tool (`memory_20250818`) for persistent file storage across sessions. Claude automatically checks memory before tasks.

**Impact for Aletheia:** This is structurally identical to our workspace file approach (MEMORY.md, CONTEXT.md). The API provides the same primitives (view, create, str_replace, insert, delete, rename) that our bootstrap currently handles manually. Using the memory tool would let Claude manage its own memory files natively rather than through custom tool calls.

**Action:** Evaluate whether to adopt the memory tool as the transport layer for our workspace files, while keeping our knowledge graph for structured recall.

### 5.3 Context Editing / Compaction

**What it is:** Server-side clearing of old tool results (`clear_tool_uses_20250919`) with configurable thresholds. Compaction summarizes entire conversations server-side.

**Impact for Aletheia:** Our `melete` distillation engine solves the same problem — context window management. Native compaction handles it at the API level without custom code. However, melete provides structured sections (Summary, TaskContext, CompletedWork, KeyDecisions, CurrentState, OpenThreads) that generic compaction doesn't.

**Action:** Use native compaction for basic context management. Keep melete for structured distillation that preserves specific information categories.

### 5.4 Structured Output (Guaranteed JSON Schema)

**What it is:** `output_config.format` guarantees responses adhere to a provided JSON schema. No parsing failures.

**Impact for Aletheia:** Our extraction pipeline (`mneme::extract`) uses custom prompts to get structured output from the LLM. Guaranteed schema adherence would eliminate parsing errors and reduce retry logic.

**Action:** Adopt structured output for all extraction operations (fact extraction, entity resolution, relationship typing).

### 5.5 1M Token Context Window

**What it is:** Beta on Opus 4.6. Standard 200K, extended to 1M with premium pricing (2x input, 1.5x output over 200K).

**Impact for Aletheia:** With 1M tokens, distillation frequency drops dramatically. A full day's session history could fit in context. However, "lost-in-the-middle" degradation still applies — performance drops for information in the center of long contexts.

**Action:** Monitor beta quality. Don't redesign around it yet — degradation at scale is real. But for recall-heavy sessions, it could reduce the need for aggressive distillation.

### 5.6 Agent Skills Open Standard

**What it is:** Portable skill definitions (SKILL.md with metadata) that work across Claude, ChatGPT, Copilot, and other platforms. Progressive disclosure — agents load only relevant skills.

**Impact for Aletheia:** Our domain packs (thesauros) and the Agent Skills standard serve overlapping purposes. Skills handle the instructional layer; packs handle tools + data. We could publish our packs as Agent Skills for cross-platform compatibility.

**Action:** Evaluate exporting thesauros packs as Agent Skills for the instructional components.

---

## 6. Emerging Threats (6-12 Month Horizon)

### 6.1 Computer Use / Desktop Agent

**Current state:** Claude's computer use capability has reached 61% success rate on OSWorld benchmarks (up from 14.9% at launch). Multi-agent desktop orchestration is imminent.

**Threat to Aletheia:** If Claude can autonomously navigate desktop applications, the need for custom tool implementations drops further. File management, web browsing, and app interaction become native capabilities.

**Timeline:** Production-ready desktop agent likely by late 2026. Aletheia's organon tools for filesystem/web become fully redundant.

### 6.2 MCP Ecosystem Maturity

**Current state:** 10,000+ active public MCP servers. Adopted by ChatGPT, Cursor, Gemini, VS Code. Donated to Linux Foundation's Agentic AI Foundation (AAIF).

**Threat to Aletheia:** MCP is becoming the universal tool protocol. Aletheia's `ToolExecutor` trait is proprietary. If we don't support MCP natively, we're locked out of the ecosystem's tools.

**Mitigation:** `rmcp` is already in our dependency list. Implement `McpProvider` as planned. Each MCP server = instant tool for every nous.

### 6.3 Agent-to-Agent Protocols

**Current state:** A2A (Agent-to-Agent) economy emerging. Google's A2A protocol, Anthropic's MCP + Agent Skills creating interoperability layers.

**Threat to Aletheia:** Our `CrossNousRouter` is internal — nous-to-nous within Aletheia. If agents need to communicate across platforms (Aletheia agent ↔ Claude Code agent ↔ external agent), we need standard protocols.

**Mitigation:** Expose CrossNousRouter via MCP or A2A protocol adapter.

### 6.4 Fine-Tuning for Claude

**Current state:** Enterprise-focused fine-tuning available for specific skills. "Agentic Service Providers" specializing in fine-tuned skills for domains (legal, medical).

**Threat to Aletheia:** If Claude can be fine-tuned for specific agent behaviors, some of our runtime configuration (SOUL.md personality, domain knowledge) could be baked into the model itself. A fine-tuned Claude that "is" Syn wouldn't need SOUL.md.

**Assessment:** Low near-term threat. Fine-tuning loses the flexibility of runtime configuration. SOUL.md can change per-session; fine-tuning cannot. But for stable, long-running agents, fine-tuning could replace some identity infrastructure.

### 6.5 Native Long-Running Agent Frameworks

**Current state:** Anthropic's engineering blog documents patterns for long-running agents: memory tool + compaction + structured recovery. These patterns converge with what Aletheia's pipeline provides.

**Threat to Aletheia:** As Anthropic publishes reference architectures for agent persistence, the "roll your own runtime" value proposition weakens. If the official SDK provides session management, memory, and context management, maintaining a 42K-line Rust runtime needs strong justification.

**Assessment:** Medium threat. The reference patterns are still building blocks, not a complete runtime. Aletheia's value is in the assembled system — knowledge graph + identity + cross-agent routing + channel integration + encrypted storage. No reference architecture combines all of these.

---

## 7. Recommendations (Prioritized)

### Tier 1: Do Now (high impact, low effort)

1. **Adopt prompt caching** in hermeneus — cache system prompts and tool definitions. Estimated 80-90% reduction in input token costs for multi-turn sessions.

2. **Adopt structured output** for mneme::extract — use `output_config.format` with JSON schema for fact/entity/relationship extraction. Eliminates parsing failures.

3. **Remove custom file/exec/web-search tools** from organon when running under Claude Code. These are dead weight. Keep them behind a feature gate for standalone API mode.

### Tier 2: Do Soon (high impact, moderate effort)

4. **Implement MCP provider** — wrap our tools as MCP servers so they're accessible to any MCP client (CC, Cursor, etc.). This also future-proofs against ecosystem lock-in.

5. **Integrate native compaction** as the default context management strategy, with melete as an optional structured distillation overlay for sessions that need it.

6. **Evaluate memory tool API** as transport for workspace files — the API's CRUD primitives match our workspace file operations. Could simplify bootstrap by letting Claude manage its own memory natively.

### Tier 3: Do When Strategic (unique value, needs investment)

7. **Invest in knowledge graph** (mneme) — this is the core differentiator. No native equivalent. Improve FSRS decay parameters, add graph visualization, expand entity resolution.

8. **Invest in prosoche** (attention system) — proactive agent behavior is unique. Expand trigger types, add external event sources (calendar, CI/CD, monitoring alerts).

9. **Invest in cross-agent topology** — as agent teams mature, our peer-to-peer routing becomes more valuable, not less. Add A2A protocol adapter for external agent communication.

10. **Publish domain packs as Agent Skills** — make thesauros packs portable across the Agent Skills ecosystem while keeping the richer tool+data bundle internally.

### Tier 4: Watch and Evaluate

11. **1M context window** — monitor beta quality. Don't redesign around it until degradation curves are well-understood.

12. **Computer use** — monitor capability growth. When desktop agent is production-ready, remove remaining organon tool stubs.

13. **Fine-tuning** — monitor availability. For stable long-running agents, a fine-tuned identity could replace SOUL.md overhead.

---

## 8. Sources

### Anthropic Official Documentation
- [Models overview](https://platform.claude.com/docs/en/about-claude/models/overview) — model capabilities, pricing, context windows
- [What's new in Claude 4.6](https://platform.claude.com/docs/en/about-claude/models/whats-new-claude-4-6) — adaptive thinking, tool search in thinking
- [Extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking) — budget control, tool use during thinking
- [Memory tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool) — client-side CRUD, security, compaction integration
- [Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) — cache tiers, pricing, breakpoints
- [Tool use overview](https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview) — parallel calling, error recovery
- [Programmatic tool calling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/programmatic-tool-calling) — server-side orchestration
- [Compaction](https://platform.claude.com/docs/en/build-with-claude/compaction) — server-side context summarization
- [Context windows](https://platform.claude.com/docs/en/build-with-claude/context-windows) — 200K standard, 1M beta, degradation notes

### Claude Code Documentation
- [Agent Teams](https://code.claude.com/docs/en/agent-teams) — lead-teammate topology, experimental
- [Custom subagents](https://code.claude.com/docs/en/sub-agents) — creation, tool access, hooks
- [Memory and project understanding](https://code.claude.com/docs/en/memory) — CLAUDE.md, Session Memory, skills

### Industry and Research
- [Introducing Claude 4](https://www.anthropic.com/news/claude-4) — model capabilities announcement
- [Advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use) — tool use patterns
- [Visible extended thinking](https://www.anthropic.com/news/visible-extended-thinking) — thinking mode details
- [Agent Skills open standard](https://thenewstack.io/agent-skills-anthropics-next-bid-to-define-ai-standards/) — portable skill definitions
- [MCP donated to Linux Foundation](https://www.anthropic.com/news/donating-the-model-context-protocol-and-establishing-of-the-agentic-ai-foundation) — AAIF establishment
- [Zep temporal knowledge graph](https://arxiv.org/abs/2501.13956) — bi-temporal graph architecture (nearest competitor to mneme)
- [Memory in the Age of AI Agents](https://arxiv.org/abs/2512.13564) — survey of agent memory architectures
- [ICLR 2026 MemAgents Workshop](https://openreview.net/pdf?id=U51WxL382H) — memory for LLM-based agentic systems
- [Context window scaling analysis](https://dasroot.net/posts/2026/02/context-window-scaling-200k-tokens-help/) — degradation at 130K+
- [Claude Code context buffer](https://claudefa.st/blog/guide/mechanics/context-buffer-management) — 33K-45K token working buffer

### Competitor Analysis
- [Best AI Coding Agents 2026](https://www.faros.ai/blog/best-ai-coding-agents-2026) — Cursor, Windsurf, Cline, Aider comparison
- [Cursor vs Windsurf vs Claude Code 2026](https://dev.to/pockit_tools/cursor-vs-windsurf-vs-claude-code-in-2026-the-honest-comparison-after-using-all-three-3gof) — honest comparison
- [Claude Code alternatives](https://www.digitalocean.com/resources/articles/claude-code-alternatives) — landscape survey
