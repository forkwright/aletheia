# Translation tax: what aletheia loses at component boundaries

> Audit of six major boundaries where rich internal state narrows into a
> channel, and what that costs. Research for #3504.

---

## 1. Recall result → LLM context

### What exists before the boundary

The recall pipeline produces `ScoredResult` values that carry six factor scores,
provenance metadata, and graph context:

- `vector_similarity`: cosine distance from HNSW search (`crates/episteme/src/recall.rs:90`)
- `decay`: FSRS power-law decay from last access time (`crates/episteme/src/recall.rs:92`)
- `relevance`: nous-specific ownership boost (`crates/episteme/src/recall.rs:94`)
- `epistemic_tier`: verified > inferred > assumed (`crates/episteme/src/recall.rs:96`)
- `relationship_proximity`: graph hop distance from query entities
  (`crates/episteme/src/recall.rs:98`)
- `access_frequency`: logarithmic access-count score
  (`crates/episteme/src/recall.rs:100`)
- `source_type`: fact, message, note, or document (`crates/episteme/src/recall.rs:109`)
- `source_id`: the knowledge-store primary key (`crates/episteme/src/recall.rs:111`)
- `sensitivity`: `Public`, `Internal`, or `Confidential`
  (`crates/episteme/src/recall.rs:121`)

The `RecallEngine` ranks candidates by weighted combination of these factors
(`crates/episteme/src/recall.rs:326`).

### What crosses the boundary

`format_section` in `crates/nous/src/recall/scoring.rs:112` renders the top-N
results as a flat markdown list:

```rust
for r in results {
    let _ = write!(out, "\n- [{:.2}] {}", r.score, r.content);
}
```

Only the final composite `score` and the `content` string survive. The six
individual factor scores, the source type, the source ID, the sensitivity
classification, and any graph proximity information are all discarded before
the LLM prompt is built.

### Tax

The LLM cannot see *why* a fact was recalled. It does not know that fact A
outranked fact B because of a higher epistemic tier, nor that fact C is a
direct graph neighbor of the query entity. This makes it impossible for the
model to weight its own reasoning by recall confidence or to request
clarification when a low-tier fact contradicts a high-tier one.

### Verdict

**Loss matters.** The factor scores exist to help the system rank memories, but
the LLM -- the consumer of those memories -- receives none of that signal.

---

## 2. Tool execution → tool result

### What exists before the boundary

Tool executors run in a rich execution context:

- `computer_use` captures before/after screenshots, computes diff regions, and
  runs inside a Landlock sandbox (`crates/organon/src/builtins/computer_use/sandbox.rs:97`).
- `view_file` resolves symlinks, validates paths against allowed roots, and
  detects media kinds (`crates/organon/src/builtins/view_file.rs:65`).
- Process-based tools have exit codes, stderr streams, timing, and sandbox
  violation logs (`crates/organon/src/sandbox/policy.rs:406`).

### What crosses the boundary

Every executor returns `ToolResult` (`crates/organon/src/types/mod.rs:332`):

```rust
pub struct ToolResult {
    pub content: ToolResultContent,
    pub is_error: bool,
}
```

`ToolResultContent` is either text, a list of blocks, or empty. The
`ComputerUseExecutor` serializes its rich `ActionResult` (which contains
`diff_region`, `change_description`, and a base64 frame) into a JSON string and
wraps it in `ToolResult::text` (`crates/organon/src/builtins/computer_use/executor.rs:120`).

During dispatch, the pipeline truncates tool results to
`max_tool_result_bytes` (`crates/nous/src/execute/dispatch.rs:36`). Text is cut
at char boundaries with a `[truncated: X -> Y bytes]` indicator. Non-text
blocks (images, documents) are skipped entirely when they exceed the remaining
budget (`crates/nous/src/execute/dispatch.rs:116`).

The `ToolCall` record that persists for metrics and loop detection stores only
`result: Some(content.text_summary())` (`crates/nous/src/execute/dispatch.rs:284`).

### Tax

- **stderr is usually lost.** Most builtin tools return `ToolResult::error(...)`
  with a single string; stderr is not captured separately.
- **Sandbox violations are invisible to the LLM.** The Landlock policy may deny
  access, but the tool result carries no trace of *what* was denied or *why*.
- **Process exit codes disappear.** A command that exits `1` and a command that
  exits `127` both become `is_error: true` with no differentiation.
- **Computer-use frames are flattened.** The before/after frames and computed
  diff region become a single JSON blob; the LLM cannot ask for a re-capture
  at a different resolution.

### Verdict

**Partially correct.** Isolating the LLM from raw process state is a security
boundary. However, the current format drops diagnostic metadata (exit code,
stderr, sandbox trace) that would help the LLM recover from tool failures
without operator intervention.

---

## 3. Cross-agent message

### What exists before the boundary

A sender agent has full session state:

- `SessionState` with turn counter, cumulative tokens, model config, bootstrap
  hash, and distillation count (`crates/nous/src/session.rs:16`)
- `PipelineContext` with conversation history, recall results, working state,
  and compaction metrics (`crates/nous/src/pipeline/mod.rs:43`)
- `WorkingState` with task stack, focus context, and wait state
  (`crates/nous/src/working_state.rs:127`)

### What crosses the boundary

`CrossNousMessage` carries exactly four strings plus metadata
(`crates/nous/src/cross/mod.rs:35`):

```rust
pub struct CrossNousMessage {
    pub from: String,
    pub to: String,
    pub target_session: String,
    pub content: String,   // <-- the only payload
    // ... delivery metadata
}
```

When the receiver handles the message, it creates a brand-new session with key
`cross:{from}` (`crates/nous/src/actor/turn.rs:410`). The comment at line 545
explains:

> "cross-nous messages carry no database session ID: generate one so finalize
> can create the DB row"

The receiver's pipeline rebuilds everything from scratch using its own config,
its own knowledge store, and an empty conversation history. No streaming
channel is attached, so the sender cannot observe tool-start or LLM-delta
events during the remote turn (`crates/nous/src/actor/mod.rs:375`).

### Tax

The sender and receiver share no conversational continuity. The receiver cannot
see that the sender has already exhausted most of its token budget, that the
sender recently distilled its session, or that the sender's working state
contains a half-finished task. Every cross-agent ask is effectively a cold
start.

### Verdict

**Correct by design.** Actor isolation prevents session leakage and circular
deadlocks (the router runs cycle detection at `crates/nous/src/cross/router.rs:154`).
However, the *complete* loss of session context means multi-agent workflows
cannot build on shared state without explicit tool-mediated persistence.

---

## 4. SSE event

### What exists before the boundary

Inside the pipeline, streaming turns produce `TurnStreamEvent`
(`crates/nous/src/stream.rs:12`):

```rust
pub enum TurnStreamEvent {
    LlmDelta(LlmStreamEvent),
    ToolStart { tool_id, tool_name, input },
    ToolResult { tool_id, tool_name, result, is_error, duration_ms },
}
```

This is a continuous, ordered stream of fine-grained state changes.

### What crosses the boundary

The Pylon handler bridges `TurnStreamEvent` into `WebchatEvent`
(`crates/pylon/src/handlers/sessions/streaming.rs:527`):

```rust
TurnStreamEvent::LlmDelta(LlmStreamEvent::TextDelta { text }) => {
    WebchatEvent::TextDelta { text }
}
TurnStreamEvent::ToolResult { tool_id, tool_name, result, is_error, duration_ms } => {
    WebchatEvent::ToolResult { tool_name, tool_id, result, is_error, duration_ms }
}
```

Each event is then serialized to JSON and emitted as a discrete SSE message
with a monotonic sequence ID (`crates/pylon/src/stream.rs:10`). The
`TurnBuffer` caps retention at `MAX_EVENTS_PER_TURN = 10_000`
(`crates/pylon/src/turn_buffer.rs:23`); beyond that, events are dropped.

### Tax

- **Inter-event timing is lost.** The client cannot tell whether a 10-second
  gap between `text_delta` events was LLM latency, tool execution, or network
  jitter.
- **Partial tool state is invisible.** If a tool streams progressive output,
  only the final `ToolResult` crosses the boundary.
- **Reconnection replays are lossy.** The buffer stores serialized JSON strings,
  not the original `TurnStreamEvent` structs, so typed consumers must re-parse.
- **Thinking deltas are schema-ready but not emitted.** `SseEvent::ThinkingDelta`
  exists in the OpenAPI schema (`crates/pylon/src/stream.rs:33`) but the
  streaming bridge does not forward thinking blocks to the webchat protocol.

### Verdict

**Partially correct.** Discrete events are the right abstraction for HTTP
clients, but the buffer limit and the lack of timing metadata make recovery
and debugging harder than necessary.

---

## 5. Distillation

### What exists before the boundary

A session may contain 100--150 messages mixing user text, assistant reasoning,
tool calls, tool results, thinking blocks, and image attachments.

### What crosses the boundary

The distillation engine (`crates/melete/src/distill.rs:359`) splits messages
into a head (to summarize) and a verbatim tail (default: last 3 messages):

```rust
let tail = self.config.verbatim_tail.min(messages.len());
let split_at = messages.len() - tail;
let to_summarize = &messages[..split_at];
let verbatim = &messages[split_at..];
```

Before summarization, near-duplicate messages are pruned via Jaccard
similarity (`crates/melete/src/distill.rs:387`). Tool results in the
summarization prompt are truncated to 500 characters
(`crates/melete/src/prompt.rs:100`). Images and other non-text content blocks
are silently skipped (`crates/melete/src/prompt.rs:79`):

```rust
_ => {
    // NOTE: other content block types not rendered in prompt summary
}
```

The LLM compresses the remaining text into seven sections targeting 400--600
words total (`crates/melete/src/prompt.rs:28`):

1. `## Summary`
2. `## Task Context`
3. `## Completed Work`
4. `## Key Decisions`
5. `## Current State`
6. `## Open Threads`
7. `## Corrections`

If the summary still exceeds the context window, the engine falls back to
dropping the oldest messages (`crates/melete/src/distill.rs:575`).

### Tax

| Rich state | What survives |
|---|---|
| Exact wording of 100+ turns | 400--600 word summary + 3 verbatim messages |
| Full tool results (potentially thousands of chars) | First 500 chars per result |
| Image attachments | Dropped entirely |
| Thinking blocks | Rendered as plain text, then summarized |
| Near-duplicate turns | Only the most recent kept |
| Custom sections | Not extracted to memory flush (`crates/melete/src/distill.rs:780`) |

The LLM that resumes a distilled session has no access to the original
reasoning chains, error traces, or exploratory tool calls that led to the
summarized decisions.

### Verdict

**Correct by design with a high tax.** Distillation exists precisely because
context windows are finite. The 500-character tool-result truncation and image
silencing are the biggest avoidable losses.

---

## 6. Knowledge extraction

### What exists before the boundary

A completed turn contains user input, assistant output, tool calls with their
full results, and any thinking blocks produced by the model.

### What crosses the boundary

The background extraction task (`crates/nous/src/actor/background.rs:74`)
constructs exactly two `ConversationMessage` values:

```rust
let messages = vec![
    ConversationMessage { role: "user".to_owned(), content: user_content.to_owned() },
    ConversationMessage { role: "assistant".to_owned(), content: assistant_content.to_owned() },
];
```

Tool calls and their results are **not included** in the extraction input. The
`ConversationMessage` type (`crates/episteme/src/extract/types.rs:99`) has only
`role` and `content` fields -- no tool-use ID, no result blocks, no error
flags, no duration metadata.

The extraction engine prompts an LLM to emit `Extraction`
(`crates/episteme/src/extract/types.rs:7`):

```rust
pub struct Extraction {
    pub entities: Vec<ExtractedEntity>,
    pub relationships: Vec<ExtractedRelationship>,
    pub facts: Vec<ExtractedFact>,
}
```

Facts are subject-predicate-object triples with a confidence score. The
refinement stage classifies the turn (`crates/episteme/src/extract/refinement.rs:76`)
and filters low-confidence facts, but the raw reasoning that produced the
assistant's content is gone.

### Tax

- **Tool-mediated reasoning is invisible.** If the assistant used three tool
calls to arrive at an answer, the extraction sees only the final answer, not
the tool chain that produced it.
- **Corrections are heuristically detected.** The system looks for patterns
like "actually, it's" (`crates/episteme/src/extract/refinement.rs:104`), but
subtle retractions embedded in tool output or reasoning blocks are missed.
- **Causal signals are downstream orphans.** `RefinedExtraction` detects causal
language ("because", "therefore") but stores only a `causal_signal` tuple
(`crates/episteme/src/extract/types.rs:125`); the full causal chain is not
reconstructed.

### Verdict

**Loss matters.** The extraction boundary discards the *process* of knowing
and keeps only the *products*. This makes the knowledge store weaker for
multi-hop reasoning, where the path between facts is as important as the facts
themselves.

---

## Summary table

| Boundary | Preserved | Lost | Verdict |
|---|---|---|---|
| Recall → LLM | `score`, `content` string | Six factor scores, source type, sensitivity, graph hops | Loss matters |
| Tool execution → result | `content`, `is_error` | Exit code, stderr, sandbox trace, frame detail | Partially correct |
| Cross-agent message | `content` string | All session state, history, working state | Correct by design |
| SSE event | JSON event per discrete change | Inter-event timing, partial state, thinking deltas | Partially correct |
| Distillation | 7 sections + 3 verbatim messages | Exact wording, images, tool results > 500 chars | Correct but costly |
| Knowledge extraction | Entities, relationships, facts | Tool chains, reasoning blocks, causal chains | Loss matters |

---

## Follow-up issues filed

See the linked issues below for concrete remediation proposals on the three
boundaries where loss is most recoverable:

1. **#3611** -- Inject recall factor metadata into LLM prompts
2. **#3612** -- Preserve tool diagnostic metadata (exit code, stderr) in tool results
3. **#3613** -- Include tool calls and reasoning blocks in knowledge extraction input
