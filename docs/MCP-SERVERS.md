# External MCP Servers

Operator-side MCP servers that extend an agent's capabilities without modifying the aletheia binary. Aletheia is a 48-crate Rust workspace plus the excluded desktop shell. Coding agents (Claude Code, Cursor, etc.) that only have grep and file reads burn tokens re-discovering structure that a Language Server could answer in one request. The servers below close that gap.

These servers run as **operator-side tooling**, not as part of the aletheia binary. They are registered in the agent's client config (e.g. `~/.claude.json`, `.mcp.json`, or Cursor's `mcp.json`). No aletheia crate depends on them, and they are not registered into `organon::registry::ToolRegistry` or served by `DiaporeiaServer`. See [Why external, not vendored](#why-external-not-vendored) below.

## MCP tool planes

| Plane | Who configures it | Hosting process | Owning crate/module |
|-------|-------------------|-----------------|---------------------|
| Operator-side MCP servers | The operator configures the agent client (Claude Code, Cursor, Windsurf, etc.) to load servers such as Serena or kanon. | The operator's agent CLI and the external MCP server process; outside the aletheia runtime. | None in aletheia. These tools are not registered into `organon::registry::ToolRegistry` and are not hosted by `DiaporeiaServer`. |
| Runtime-bridged MCP tools | The deployment configures `[tools]` entries in `aletheia.toml`; MCP entries require the `mcp` feature. | The aletheia runtime connects as an MCP client to configured external MCP servers, discovers `tools/list`, registers discovered tools into `organon::registry::ToolRegistry`, and calls back through MCP `tools/call`. | `crates/aletheia/src/external_tools.rs` owns the bridge; `crates/diaporeia/src/client` provides the MCP client types; `organon` owns in-process dispatch through `ToolRegistry`. |
| DiaporeiaServer-exposed MCP tools | The deployment configures Aletheia's MCP/gateway settings and external clients connect to Aletheia. | The aletheia process hosts `DiaporeiaServer` over stdio (`aletheia mcp`) or streamable HTTP at `/mcp`. | `crates/diaporeia` owns the exposed server surface through `DiaporeiaServer` and its `rmcp::ToolRouter<Self>`; these tools are intentionally separate from `organon::registry::ToolRegistry`. |

Any new MCP integration must state which plane it lives on before claiming tool availability in the `nous` loop, the Diaporeia server surface, or operator-local agent tooling.

Runtime-bridged MCP tool annotations are untrusted by default. A discovered
tool's `readOnlyHint` does not lower approval requirements unless the
operator sets `trustAnnotations = true` on that specific `[tools.required.*]`
or `[tools.optional.*]` MCP server entry. Use that opt-in only for servers
inside the deployment's trust boundary.

## Index

| Server | Purpose | Transport | Install |
|--------|---------|-----------|---------|
| [Serena](#serena-lsp-powered-code-navigation) | LSP-backed symbol navigation (go-to-def, find-refs, rename) | stdio | `uv tool install` |

## Serena: LSP-powered code navigation

[Serena](https://github.com/oraios/serena) wraps `rust-analyzer` (and 40+ other language servers) as MCP tools. For aletheia, this means an agent can ask:

- **`find_symbol`** - locate a symbol by name/path across the workspace crates.
- **`find_referencing_symbols`** - find every caller of a trait/fn/type across the workspace.
- **`get_symbols_overview`** - get the top-level symbols defined in a file without reading it.
- **`rename_symbol`** - workspace-wide safe rename via LSP refactoring.
- **`replace_symbol_body`** / **`insert_before_symbol`** / **`insert_after_symbol`** - structured edits at symbol boundaries instead of fragile line/char patches.

This is the same navigation an IDE user has, exposed as MCP.

### Install

```bash
uv tool install -p 3.13 'serena-agent@latest' --prerelease=allow
```

This installs the `serena` and `serena-hooks` entry points. The `--prerelease=allow` flag is required by upstream install instructions to pick up the current release line.

`rust-analyzer` is auto-fetched by Serena on first activation - no separate install step is required.

### Register with claude code

From the aletheia workspace root:

```bash
./scripts/serena-mcp.sh register
```

This is equivalent to:

```bash
claude mcp add serena -- serena start-mcp-server \
    --context claude-code \
    --project "$(pwd)" \
    --enable-web-dashboard false \
    --open-web-dashboard false
```

For a user-level registration usable from any project (uses the current working directory at tool invocation time):

```bash
claude mcp add --scope user serena -- serena start-mcp-server \
    --context claude-code \
    --project-from-cwd \
    --enable-web-dashboard false \
    --open-web-dashboard false
```

### Start the server manually (debug / non-Claude clients)

```bash
./scripts/serena-mcp.sh start
```

This foregrounds a stdio MCP server rooted at the aletheia workspace. Useful for:

- Manually speaking JSON-RPC to validate the server (see [Smoke test](#smoke-test)).
- Registering with Cursor, Windsurf, or any other MCP-capable client that expects a raw command.

### On first start

Serena auto-creates `.serena/` in the workspace root with `project.yml` (language detection - 70% Rust for aletheia), a `cache/` for the LSP index, and a `memories/` folder for Serena's own project notes. The `.serena/` directory is gitignored - it is operator-local state, not repo content.

### Smoke test

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.1"}}}\n' \
  | ./scripts/serena-mcp.sh start 2>/dev/null \
  | head -1
```

Expect a JSON-RPC response advertising `tools/list`, `resources/list`, etc. The server logs go to `~/.serena/logs/<date>/mcp_<timestamp>.txt`.

### Tools exposed in `claude-code` context

The `claude-code` context (upstream renamed from the deprecated `ide-assistant`) narrows Serena's 44-tool surface to 17 navigation/edit tools - file I/O, shell, and pattern-search tools are suppressed because Claude Code already provides those. Resulting active tools:

```
check_onboarding_performed, find_referencing_symbols, find_symbol,
get_symbols_overview, initial_instructions, insert_after_symbol,
insert_before_symbol, list_memories, onboarding, read_memory,
rename_memory, rename_symbol, replace_symbol_body, safe_delete_symbol,
write_memory, edit_memory, delete_memory
```

### Acceptance checks against this workspace

From the worktree root, after registering the server in Claude Code:

1. **Cross-crate go-to-definition** - ask the agent to `find_symbol` for `ToolExecutor` (defined in `organon`) and confirm implementors in `organon/builtins/*` are reachable.
2. **Workspace-wide find-references** - `find_referencing_symbols` for `AgentId` (defined in `koina`) should return hits across `nous`, `pylon`, `energeia`, etc.
3. **Symbol overview** - `get_symbols_overview` for `crates/nous/src/pipeline/mod.rs` returns top-level items without reading the file.

## Why external, not vendored

An earlier draft of the integrating issue (#3355) proposed wiring Serena into the `diaporeia` tool bus as a vendored MCP client. We chose the external route instead:

- **Lower blast radius.** Zero aletheia crate changes, zero new dependencies in `Cargo.toml`. The server lives outside the Rust workspace.
- **Upstream stays upstream.** Serena releases often; vendoring would mean tracking their schema and prompts in-tree. Operators get upstream changes via `uv tool upgrade serena-agent` with no aletheia release required.
- **Agents already have an MCP client.** Claude Code, Cursor, and Windsurf all speak MCP natively. Aletheia's internal tool loop uses `organon::registry::ToolRegistry` for tools the `nous` pipeline calls, not for operator-side coding helpers.
- **Sovereignty path preserved.** If the upstream trajectory diverges from our needs, a Rust-native MCP server wrapping `rust-analyzer` directly can be added under `crates/` later. This is tracked as a follow-up (see PR body for #3355).

## See also

- `scripts/serena-mcp.sh` - wrapper for registering and starting Serena against this workspace.
- Serena upstream: https://github.com/oraios/serena
- Serena client docs: https://oraios.github.io/serena/02-usage/030_clients.html
