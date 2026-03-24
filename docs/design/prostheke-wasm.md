# prostheke: WASM plugin host design

Planning document for the `prostheke` crate (M5). Merges the original TypeScript-era
plugin design (`PLUGINS-DESIGN.md`, removed in `8ace85eb4`) with patterns from Zed's
WASM extension architecture. This is not an implementation spec; it captures decisions,
superseded assumptions, and open questions before implementation starts.

---

## Context

`prostheke` is the M5 WASM plugin host. It sits between the agent runtime and third-party
extensions, giving plugin authors a sandboxed environment to register tools and hook into
the agent lifecycle without modifying core crates.

The current extension mechanism (domain packs via `thesauros`) handles static context
injection and shell-command tools but cannot run arbitrary logic in response to agent
events. `prostheke` fills that gap.

---

## What carries forward from the original design

The TypeScript plugin architecture established a lifecycle model that remains correct:

### Lifecycle hooks

The four-hook model is the right shape:

| Hook | When | What it can do |
|------|------|----------------|
| `before_agent_start` | Before each turn | Inject context, recall external memory |
| `agent_end` | After each turn | Extract facts, log metrics, sync state |
| `on_tool_result` | After a tool executes | Transform or annotate output |
| `on_message` | Message arrives | Pre-process, filter, route |

The `before_agent_start` return value (an optional string to inject into context) is a
clean pattern. It lets plugins augment the agent's working memory without owning the
context assembly pipeline.

### Custom tool registration

Plugins register tools at load time. The tool definition (name, description, JSON Schema
for input) feeds into the existing `ToolRegistry`. The plugin provides the executor; the
registry owns dispatch. This boundary survives the JS-to-WASM migration unchanged.

### Per-plugin config

Each plugin receives its config slice at init time. The host reads it from `aletheia.toml`
and passes it to the plugin's init hook. This is a cleaner contract than environment
variables.

### Path-based discovery

Plugins load from filesystem paths declared in config. A package registry is a future
concern. Local paths first.

---

## Where Zed patterns supersede the original design

### WIT interface instead of JSON/JS dispatch

The original design used a JavaScript manifest (`manifest.json`) and a JS entry point
(`index.js`) with dynamic dispatch via duck-typed hook objects. This is not applicable
to a WASM host.

Zed's extension model uses WebAssembly Interface Types (WIT) to define a typed host/guest
boundary. The host and guest agree on an interface at compile time. There is no runtime
type negotiation.

For `prostheke`, this means:

- The plugin ABI is a WIT interface definition in the `prostheke` crate
- Guest plugins are Rust crates compiled to `wasm32-wasip2` (WASM component model)
- A `prostheke-sdk` crate provides the Rust trait + derive macro for plugin authors
- The host (wasmtime with component model support) instantiates the WASM component and
  calls its exported functions

The WIT surface replaces the JSON manifest + JS export object.

### Capability-based sandbox

The original design had no sandbox. JavaScript plugins ran in the same process with full
access to the host environment.

WASM provides isolation by default. A plugin cannot access the filesystem, network, or
host memory except through explicit host functions. `prostheke` grants capabilities via
the WASM component model's import/export mechanism: if the host does not export a
function, the plugin cannot call it.

This is Zed's model: extensions can make HTTP requests only because the host exports an
HTTP function with its own rate limiting and allow-listing. Extensions cannot open raw
sockets.

For `prostheke`, the initial capability set is:

| Capability | Granted | Notes |
|------------|---------|-------|
| Inject context string | Yes | Return from `before_agent_start` |
| Register tools | Yes | Declared in plugin manifest WIT |
| Read plugin config | Yes | Passed at init |
| Access KnowledgeStore | No | M6 consideration |
| Outbound HTTP | No | M6 consideration |
| Filesystem access | No | Shell tool packs cover this use case |

### Rust SDK over raw WASM

Zed extensions are Rust crates. Plugin authors implement a single trait and annotate it
with a derive macro that generates the WASM component boilerplate. They never write WIT
directly.

`prostheke` should follow the same pattern. The `prostheke-sdk` crate (`wasm32` target
only) exposes:

```rust
pub trait Plugin: Sized {
    fn init(config: PluginConfig) -> Result<Self>;
    fn before_agent_start(&self, ctx: TurnContext) -> Option<String>;
    fn agent_end(&self, ctx: TurnContext);
    fn on_tool_result(&self, result: ToolResult) -> ToolResult;
    fn on_message(&self, msg: Message) -> Message;
    fn tools(&self) -> Vec<ToolDefinition>;
}
```

Default implementations return `None` / pass through / empty vec so authors only
implement the hooks they use.

### Component model over module linking

The original design loaded plugins as Node.js modules. WASM module linking (shared
memory, table linking) is an older model with limited tooling support.

Zed uses the WASM component model (`wasm32-wasip2`, component-model feature in
wasmtime). This is the correct target for `prostheke`. Components are self-describing,
have defined interface boundaries, and compose without shared memory.

The wasmtime `component` API (`wasmtime::component::Component`, `Linker`, `bindgen!`)
is the implementation path.

---

## Gaps between original design and current architecture

These items appear in the original design or are implied by it but are not addressed in
any current `prostheke` planning:

### 1. Plugin discovery and versioning

The original design loaded plugins from paths in config. This works for a single
operator but gives no answer for:

- How does a user find third-party plugins?
- How are plugin versions pinned?
- What happens when a plugin targets an older `prostheke` ABI?

The WIT interface version needs to be part of the ABI contract. A plugin compiled against
`prostheke-sdk` v1 should fail cleanly against a v2 host, not silently misbehave.

Minimum viable answer: host checks the WIT interface version exported by the component
against the current host version. Version mismatch is a load error with a clear message.

### 2. Hot-reload for development

The original design implies plugins reloaded on file change (common in JS plugin hosts).
WASM components instantiate from compiled artifacts. A `watch` mode that recompiles and
reloads during development requires a file watcher, a recompilation step, and runtime
re-instantiation of the component.

Minimum viable answer: reload on config reload (`SIGHUP` or `aletheia reload`), not on
file change. Document the compile-test cycle for plugin authors.

### 3. Async host calls from guest

WASM component model async support (the `async` WIT modifier) is experimental in
wasmtime. Hook implementations that need to do async work (e.g., a `before_agent_start`
that queries an external service) cannot use async without this.

The original JS design was naturally async. WASM sync-only is a regression for these
use cases.

Options:
- Block on a host-provided sync HTTP call (no async required in guest)
- Use wasmtime's async component support when it stabilizes
- Defer async plugins to a later milestone

### 4. Plugin error isolation

If a plugin panics, the WASM runtime traps. The host must recover from a trap without
crashing the agent. The original JS design had no explicit error handling contract.

The wasmtime `Store` and `call` APIs return `Result<_, Trap>`. The host must handle
traps, log them, and continue without the failed hook's output. This needs to be
specified: does a panicking plugin get unloaded? Does it get retried? Does it get
disabled for the session?

### 5. Tool executor async

Built-in tools implement `ToolExecutor` with an async `execute` method. Plugin-provided
tools need the same contract but WASM components cannot directly implement a Rust async
trait.

The bridge: the host calls the plugin's tool executor synchronously (blocking on a
tokio `spawn_blocking`), the plugin returns a JSON string, and the host wraps it as a
`ToolResult`. This is the same stdio pattern domain packs already use. It works but
adds latency on each plugin tool call.

### 6. Debugging and observability for plugin authors

The original design had no answer for how plugin authors debug their code. WASM
components run in an opaque sandbox.

The host should forward `wasi:logging` writes from the guest to the structured log at
`debug` level, tagged with the plugin name. This lets plugin authors use `log::debug!`
in their Rust code and see output in the Aletheia log stream.

### 7. Binary size budget per plugin

No size limit is defined. A plugin that bundles a large model or dataset would load
without warning.

A reasonable initial limit: 50MB per WASM component. Configurable per plugin in
`aletheia.toml`. Load failure with a clear message if exceeded.

---

## Open questions

1. **ABI stability cadence.** How often will the WIT interface change? Pre-1.0, any
   minor version may break the ABI. Do we accept this and document it, or commit to
   stability earlier?

2. **Shared types between SDK and host.** `TurnContext`, `ToolResult`, `Message` need
   to be representable in WIT. How much of the existing type hierarchy maps cleanly to
   WIT types? What needs to be simplified for the boundary?

3. **KnowledgeStore access.** The original memory plugin (`aletheia-memory`) was the
   primary motivator for the plugin system. It needed to inject recalled memories before
   each turn and extract new facts after. Recall is now a built-in pipeline (M1). Does
   the plugin system still need KnowledgeStore access, or is the built-in recall
   sufficient? If yes, what read/write surface is exposed?

4. **Signal and TUI hooks.** The original design had `on_message` for routing. For
   Signal and TUI, does the same `on_message` hook apply, or are channel-specific hooks
   needed?

5. **Plugin composition.** Multiple plugins loaded at once. If two plugins both
   implement `before_agent_start`, their injected context strings are concatenated.
   Is that the right merge strategy? What about `on_tool_result` where two plugins
   both want to transform the same result?

---

## See also

- `crates/thesauros/` — current domain pack mechanism (`prostheke` extends this, not
  replaces it)
- `docs/PACKS.md` — domain pack reference
- `docs/ARCHITECTURE.md` — M5 milestone context
- `docs/PROJECT.md` — milestone map
- Original design source: `git show 8ace85eb4^:docs/PLUGINS-DESIGN.md`
