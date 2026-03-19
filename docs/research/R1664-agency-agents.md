# R1664: Mine Claude Agent SDK for Applicable Patterns

**Date:** 2026-03-19
**Author:** Research agent
**Status:** Final
**Closes:** #1664

---

## Executive Summary

There is no repository called "agency-agents" in the Anthropic org. The correct target is the **Claude Agent SDK** (formerly claude-code-sdk): Python (`anthropics/claude-agent-sdk-python`), TypeScript (`anthropics/claude-agent-sdk-typescript`), and demos (`anthropics/claude-agent-sdk-demos`). The SDK documents Anthropic's own reference patterns for the agentic loop, tool use, multi-agent coordination, and streaming. Several patterns are directly applicable to aletheia and represent best-practice alignment with Anthropic's production architecture.

**Seven applicable patterns identified.** Three are high-priority additions (TurnTermination taxonomy, read-only tool parallelism, correlation_id on cross-nous messages). Four are medium priority (effort level field, stream lifecycle events, async hook path, per-session tool deny-list enforcement).

---

## 1. Repository Identification

The issue refers to "agency-agents repo." After search, the correct repositories are:

| Repo | URL |
|---|---|
| Python SDK | `github.com/anthropics/claude-agent-sdk-python` |
| TypeScript SDK | `github.com/anthropics/claude-agent-sdk-typescript` |
| Demo applications | `github.com/anthropics/claude-agent-sdk-demos` |
| Docs | `platform.claude.com/docs/en/agent-sdk/overview` |

No "agency-agents" repository exists under the Anthropic org or as a notable third-party project. The SDK is the authoritative source for Anthropic's agent architecture patterns.

---

## 2. Agent Architecture

### 2.1 SDK agentic loop as a typed async stream

The SDK exposes the entire agent loop as an async iterator. Each yield is a typed message:

| Message type | Trigger |
|---|---|
| `SystemMessage { subtype: "init" }` | Loop start |
| `SystemMessage { subtype: "compact_boundary" }` | History compaction point |
| `AssistantMessage` | LLM turn (may contain tool calls and text) |
| `UserMessage` | Tool results injected back |
| `StreamEvent` | Partial deltas within a turn |
| `ResultMessage` | Terminal — contains cost, usage, stop_reason, num_turns |

**Current aletheia pattern**: `NousActor` receives `NousMessage` via bounded `mpsc`, runs pipeline stages (bootstrap → recall → execute → finalize), writes back via `oneshot`. Streaming via `TurnStreamEvent` channel. Structurally equivalent but `TurnResult` lacks the terminal message taxonomy.

### 2.2 `ResultMessage` taxonomy — `TurnTermination` pattern

The SDK's `ResultMessage.subtype` distinguishes six terminal states:

| Subtype | Cause |
|---|---|
| `success` | Normal `end_turn` |
| `error_max_turns` | `maxTurns` limit hit |
| `error_max_budget_usd` | Cost budget exceeded |
| `error_during_execution` | API failure or cancellation |
| `error_max_structured_output_retries` | Structured output failed after N retries |

`stop_reason` (`end_turn`, `max_tokens`, `refusal`) further qualifies the model-side completion.

**Applicable pattern for aletheia**: Add a `TurnTermination` enum to `crates/nous/src/pipeline/mod.rs` as a field on `TurnResult`. Currently callers (`pylon` SSE handlers) must pattern-match on snafu error variants to infer termination cause. A `TurnTermination` field makes recovery logic explicit:
- `ErrorMaxTurns` → caller may resume with raised limit
- `ErrorDuringExecution` → retry with backoff
- `Refusal` → surface to user, no retry
- `Success { stop_reason }` → log cost and move on

### 2.3 Effort levels

The SDK has `effort: "low" | "medium" | "high" | "max"` per query, controlling reasoning depth and budget. Aletheia has no equivalent. A `reasoning_effort: Option<ReasoningEffort>` field in `PipelineConfig` (or per-session `NousConfig`) would let coordinators spawn low-effort subagents for classification tasks and high-effort agents for complex synthesis — directly reducing token cost.

---

## 3. Tool Use Patterns

### 3.1 Read-only tool parallelism

The SDK distinguishes read-only tools (can run concurrently) from stateful tools (must run sequentially). Read-only tools: `Read`, `Glob`, `Grep`, and any MCP tool marked `readOnly: true`. Stateful tools: `Edit`, `Write`, `Bash`.

**Applicable pattern for aletheia**: Add `read_only: bool` to `ToolDef` (alongside existing `auto_activate: bool`). In `crates/nous/src/execute/dispatch.rs`, use `JoinSet` to fan out tool calls where all requested tools are read-only. For mixed-tool requests, serialize the stateful ones while parallelizing read-only ones.

This is a direct performance win for common turn patterns like `Glob + Grep + Read` (currently serialized, could run in parallel).

### 3.2 Per-query tool allowlists and denylists

The SDK has `allowedTools` (auto-approve list) and `disallowedTools` (block list) as per-query options. Aletheia has `active_tools: HashSet<ToolName>` in `ToolContext` but enforce is partial.

**Applicable pattern**: Make deny-lists explicit as `denied_tools: HashSet<ToolName>` in session config, propagated into `ToolContext`. Enforced unconditionally in `registry.execute()` before the executor runs. This is the pattern for safely restricting coordinator-spawned subagents.

### 3.3 Permission modes

The SDK defines four permission modes: `default`, `acceptEdits`, `plan`, `bypassPermissions`. The `plan` mode is particularly relevant: tools are called but file mutations are blocked, allowing a dry-run planning pass. Aletheia's sandbox module in `organon` implements path restrictions; mapping these to explicit modes in session config would give operators a clean control surface.

---

## 4. Memory and Knowledge

### 4.1 On-demand tool schema injection

The SDK documents that tool schema tokens are a significant per-request cost driver. It provides `ToolSearch` — a meta-tool that loads other tool schemas on demand rather than injecting all of them into every request.

**Applicable pattern for aletheia**: The `ToolRegistry` currently injects all active-tool schemas into every bootstrap. With 33 tools, this is non-trivial. An on-demand loading approach: inject only a compact tool index at bootstrap; let the model call a `tool_search` built-in to load full schema details for tools it intends to use. This reduces per-turn token cost proportionally to the fraction of tools actually needed per turn.

### 4.2 Skill summaries at bootstrap (pattern confirmation)

The SDK's `.claude/skills/SKILL.md` pattern: load short skill summaries at bootstrap, inject full content only when invoked. Aletheia already has `seed-skills`, `export-skills`, and `SkillLoader` in the actor. The SDK pattern confirms this design is correct. The optimization remaining: ensure `BootstrapSection` for skills uses `truncatable: true` and `Optional` priority so skills degrade gracefully under token pressure.

---

## 5. Multi-Agent Coordination

### 5.1 `correlation_id` on cross-nous messages

The SDK attaches a `parent_tool_use_id` to messages emitted inside a subagent context, enabling the parent's caller to route messages by origin and correlate subagent results back to the originating tool call.

**Applicable pattern for aletheia**: `CrossNousMessage` in `crates/nous/src/cross/` has `from`/`to` routing but no correlation back to the originating `ToolCall.id`. Adding `correlation_id: Option<Ulid>` (carrying the originating tool call's ID) lets `pylon` correctly attribute cross-nous responses in the SSE stream and lets TUI clients display subagent results under the correct tool call in the conversation tree.

### 5.2 Dynamic per-session agent configuration

The SDK shows a factory pattern: `AgentDefinition` constructed at query time based on runtime conditions (e.g., choose `opus` for security review, `haiku` for classification). In aletheia, `NousConfig` is currently fixed per actor identity. Supporting per-session config overrides would enable ephemeral task-specialized agents spawned by a coordinator nous without registering a new actor.

### 5.3 Tool restriction into cross-nous invocations

The SDK allows a parent to give a subagent a strict subset of its tools. Aletheia's `ToolRegistry` is `Arc`-shared; per-session tool filtering exists in `active_tools` but may not be propagated into cross-nous invocations. Ensuring the spawning coordinator's tool restrictions are inherited by spawned agents prevents privilege escalation through agent composition.

---

## 6. Streaming

### 6.1 Extended `TurnStreamEvent` lifecycle events

The SDK multiplexes session lifecycle events into the same stream as content events. Aletheia's `TurnStreamEvent` covers only turn-level events (`LlmStreamDelta`, `ToolStart`, `ToolResult`, `TurnComplete`).

**Proposed additions**:
```rust
// crates/nous/src/stream.rs
TurnStreamEvent::SessionInit { session_id: SessionId },
TurnStreamEvent::CompactionBoundary { summary_tokens: u32 },
TurnStreamEvent::SubagentStart { from: NousId, correlation_id: Ulid },
TurnStreamEvent::SubagentResult { from: NousId, correlation_id: Ulid, turn_count: u32 },
```

These let TUI clients render real-time coordination topology (which agent is active, when compaction occurs) without polling `/api/v1/sessions/{id}`.

### 6.2 Async (non-blocking) tool side-effects

The SDK distinguishes synchronous hooks (block the loop, can deny/transform) from async hooks (fire-and-forget, cannot block). Currently all tool execution in `dispatch.rs` blocks the turn, including observability side-effects like audit logging and metrics emission.

**Applicable pattern**: Emit audit and telemetry side-effects via a `tokio::spawn` within dispatch rather than awaiting them inline. This reduces turn latency for the common case where the side-effect is not load-bearing. Errors in fire-and-forget tasks are caught and logged, never propagated to the turn result.

---

## 7. Error Handling

### 7.1 `TurnTermination` enum (detail)

```rust
// Proposed addition: crates/nous/src/pipeline/mod.rs
#[derive(Debug, Clone)]
pub enum TurnTermination {
    Success { stop_reason: StopReason },
    ErrorMaxTurns { turns_used: u32, limit: u32 },
    ErrorBudgetExceeded { cost_usd: f64, budget_usd: f64 },
    ErrorDuringExecution { retryable: bool },
    ErrorRefusal,
}

#[derive(Debug, Clone)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Refusal,
    ToolUse,
}
```

`TurnResult` grows a `termination: TurnTermination` field. `pylon`'s SSE handler emits this as a structured `turn_end` event rather than mapping snafu error variants to HTTP status codes.

### 7.2 Hook error isolation (pattern confirmation)

The SDK specifies that hook errors must never propagate to the agent loop. Aletheia's background tasks (`JoinSet` in the actor) already handle panics via `DEGRADED_PANIC_THRESHOLD`. The SDK pattern confirms this architecture is correct. Verify that extraction, distillation, and audit log tasks are all in the `JoinSet` with `abort_on_panic: false` semantics.

---

## 8. Prioritized Adoption Plan

| Pattern | Target location | Effort | Value |
|---|---|---|---|
| `TurnTermination` enum + `stop_reason` | `crates/nous/src/pipeline/mod.rs` + `crates/pylon/` | Low | High |
| `read_only: bool` on `ToolDef` + parallel dispatch | `crates/organon/src/types.rs` + `crates/nous/src/execute/dispatch.rs` | Medium | High |
| `correlation_id` on `CrossNousMessage` | `crates/nous/src/cross/` | Low | Medium |
| `TurnStreamEvent` lifecycle extension | `crates/nous/src/stream.rs` | Low | Medium |
| `reasoning_effort` in `PipelineConfig` | `crates/nous/src/config.rs` | Low | Medium |
| Tool deny-list enforcement in cross-nous | `crates/nous/src/cross/` + `crates/organon/src/registry.rs` | Medium | Medium |
| Async fire-and-forget for audit side-effects | `crates/nous/src/execute/dispatch.rs` | Medium | Medium |

---

## 9. Sources

- Claude Agent SDK Overview: `platform.claude.com/docs/en/agent-sdk/overview`
- Agent Loop Architecture: `platform.claude.com/docs/en/agent-sdk/agent-loop`
- Subagents: `platform.claude.com/docs/en/agent-sdk/subagents`
- Hooks: `platform.claude.com/docs/en/agent-sdk/hooks`
- Sessions: `platform.claude.com/docs/en/agent-sdk/sessions`
- Custom Tools: `platform.claude.com/docs/en/agent-sdk/custom-tools`
- `github.com/anthropics/claude-agent-sdk-python`
- `github.com/anthropics/claude-agent-sdk-typescript`
- `github.com/anthropics/claude-agent-sdk-demos`
