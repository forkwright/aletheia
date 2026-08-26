# Domain packs

A domain pack bundles knowledge, tools, and configuration overlays that extend an Aletheia agent without modifying the core runtime. Packs keep domain-specific content (company IP, schemas, runbooks) separate from generic agent infrastructure.

## Directory structure

```text
my-pack/
  pack.toml              # Manifest (required)
  context/               # Markdown files injected into bootstrap
    BUSINESS_LOGIC.md
    GLOSSARY.md
  tools/                 # Shell scripts exposed as LLM tools
    query_database.sh
    lookup_schema.sh
```

## Configuration

Declare packs in `aletheia.toml`:

```toml
packs = [
    "/path/to/my-pack",
    "/path/to/another-pack",
]
```

Relative paths resolve from the instance root (`$ALETHEIA_ROOT` or `./instance`). Absolute paths are used as-is. Packs load at startup. Invalid or missing packs log warnings and are skipped (graceful degradation); the structured [pack health](#pack-health) report records exactly what was skipped or failed.

## Manifest: pack.toml

```toml
name = "my-domain-pack"
version = "1.0"
description = "Optional description of this pack"

[[context]]
path = "context/BUSINESS_LOGIC.md"
priority = "important"
agents = ["analyst"]

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[[tools]]
name = "query_database"
description = "Run a read-only SQL query against the data warehouse"
command = "tools/query_database.sh"
timeout = 60000
groups = ["read"]
tags = ["recon", "fetch"]
reversibility = "fully_reversible"

[tools.input_schema]
required = ["sql"]

[tools.input_schema.properties.sql]
type = "string"
description = "SQL query to execute"

[overlays.analyst]
domains = ["healthcare", "sql"]
```

### Design note: startup and on-demand references

`pack.toml` is the repo-local knowledge manifest for a domain pack. It should make the loader able to distinguish content needed at startup from material that should stay discoverable without entering every prompt.

| Reference kind | Current manifest field | Use for | Loading behavior |
|----------------|------------------------|---------|------------------|
| Startup context | `[[context]]` | Small, high-signal guidance the agent needs before a turn starts | Inject into bootstrap according to priority and token budget |
| Callable capability | `[[tools]]` | Scripts or commands the model can call when a task needs them | Register in the tool registry; do not inject tool output at startup |
| Agent routing | `[overlays.<agent>]` | Domain tags and per-agent targeting | Merge with agent domains before section filtering |
| On-demand reference | Proposed future `[[reference]]` shape | Larger runbooks, schemas, reports, or corpora | Index path and metadata, load only when recall or a tool asks for it |

A future `[[reference]]` entry should include at least `path`, `title`, `description`, and `tags`. Optional fields such as `freshness`, `owner`, `format`, and `load_hint = "startup" | "on_demand"` can help runtime loaders choose between prompt injection, search indexing, and tool-mediated retrieval.

### Load-time validation

The manifest contract is validated when the pack loads, before anything activates:

- `name` is 1–64 ASCII alphanumeric/hyphen characters; `version` is non-empty
- every tool has a valid tool name (alphanumeric, hyphens, underscores), a non-zero `timeout`, and a `command` that is a relative path inside the pack root
- every overlay's `agency` is one of `unrestricted`, `standard`, `restricted`; `model` and `system_prompt_additions` entries are not blank

All problems are reported together in one `invalid manifest` error; a pack with any violation is not loaded at all (reported as `failed` in [pack health](#pack-health)). Tool `groups`, `tags`, and `reversibility` are validated against the runtime's known values at tool registration — a failure there skips that tool and marks the pack `degraded` instead of rejecting the whole pack.

## Context entries

Each context entry maps to a file injected into the agent's system prompt at startup.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `path` | string | required | Path relative to pack root |
| `priority` | string | `important` | Bootstrap priority: `required`, `important`, `flexible`, `optional` |
| `agents` | list | `[]` (all) | Agent IDs or domain tags that receive this section |
| `truncatable` | bool | `false` | Whether the section can be trimmed under token budget pressure |

Priority controls inclusion order when the token budget is tight:
- **required**: Always included. Missing required files cause errors
- **important**: Included after required. Dropped only if budget is exhausted
- **flexible**: Truncated to fit if budget is tight
- **optional**: First to be dropped when space runs out

The `agents` field filters which agents receive the section. An empty list means all agents. Values match against both agent IDs (e.g., `analyst`) and domain tags (e.g., `healthcare`).

## Tool definitions

Tools are shell commands exposed to the LLM as callable functions. The runtime pipes JSON to stdin and reads JSON from stdout.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Tool name (alphanumeric + underscores) |
| `description` | string | required | Short description sent to the LLM |
| `command` | string | required | Path to script, relative to pack root |
| `timeout` | int | `30000` | Execution timeout in **milliseconds** |
| `groups` | list | `["command"]` | Tool gating groups: `read`, `edit`, `command`, `mcp`, `spawn_subtask`, `plan`, `verify` |
| `tags` | list | `["execute"]` | Operational tags: `recon`, `edit`, `verify`, `fetch`, `spawn`, `plan`, `execute`, `format` |
| `reversibility` | string | `irreversible` | One of `fully_reversible`, `reversible`, `partially_reversible`, `irreversible` |
| `env` | list | `[]` | Environment variable names to pass through from the daemon's environment (see [Environment and secrets](#environment-and-secrets)) |
| `write_paths` | list | `[]` | Additional directories the tool may write to, relative to the pack root |
| `egress` | string | `inherit` | Network egress intent: `inherit` (deployment sandbox policy applies) or `none` (deny outbound network for this tool) |
| `platforms` | list | `["unix"]` | Host platforms the tool supports: `linux`, `macos`, `windows`, `unix`. A tool whose list excludes the current host is skipped at registration and the pack is marked degraded |
| `input_schema` | object | none | JSON Schema for input parameters |

Input schema properties support types: `string`, `number`, `integer`, `boolean`, `array`, `object`. Each property has a `description` field and optional `enum` and `default` values.

### Tool execution flow

1. LLM emits a `tool_use` block with JSON arguments
2. Runtime serializes arguments to JSON and pipes to the command's stdin
3. Command writes result to stdout (text or JSON)
4. Runtime captures stdout as the tool result
5. Stderr content is never copied into the model-visible result or diagnostics. Operators receive only structured warning metadata (tool, exit code, byte count); arbitrary subprocess stderr cannot be made safe for the model boundary by pattern redaction
6. Output is truncated at 50KB

Diagnostics also record the exit code, wall-clock duration, and — when the sandbox itself refuses to start the command — a `sandbox_violations` entry with the setup denial reason, distinguishing "sandbox refused" from "command failed".

### Security

- Command paths are resolved relative to the pack root and canonicalized
- Paths that resolve outside the pack root are rejected (no traversal)
- No shell interpolation: commands receive input only via stdin
- Tools are registered with category `Domain` in the tool registry
- The subprocess environment is cleared except for a small safe allowlist (`PATH`, `HOME`, `TERM`, ...) plus whatever the tool explicitly declares in `env`
- The sandbox grants the tool read access to its pack root and exec access to its command; `write_paths` is the only way to add write grants, and `egress = "none"` can only tighten the deployment's network policy, never loosen it

### Environment and secrets

Tool subprocesses start with a cleared environment. A tool that needs a value from the outside — a database URL, an API token — must declare the variable *name* in `env`; the value comes from the daemon's own environment, never from `pack.toml`:

```toml
[[tools]]
name = "run_query"
description = "Execute a read-only SQL query"
command = "tools/query.sh"
env = ["DATABASE_URL"]
```

The operator provides the value on the daemon (for example a systemd `EnvironmentFile`); it never enters the pack, the manifest, or the LLM-visible tool schema. A declared variable that is absent from the daemon environment fails tool registration and degrades the pack's [health](#pack-health) — the tool never runs with a silently missing value.

### Platform support

Pack tools are shell scripts executed directly via their shebang line, so they are Unix-first by default. The `platforms` field makes a tool's support explicit:

- **Linux**: full enforcement — wall-clock timeout, process-group kill on timeout, and `RLIMIT_NPROC`/`RLIMIT_CPU` resource limits all apply
- **macOS and other Unix**: timeout and process-group kill apply; resource limits are a no-op. The startup health report notes the reduced enforcement
- **Windows**: `platforms = ["unix"]` (the default) does not cover it, so the tool is skipped at registration unless it declares `windows`

Example for a Linux-only tool:

```toml
[[tools]]
name = "gpu_probe"
description = "Probe NVIDIA GPU state"
command = "tools/gpu_probe.sh"
platforms = ["linux"]
```

## Overlays

Overlays assign per-agent domain tags and — with explicit operator opt-in — high-impact overrides. A context section tagged `agents = ["healthcare"]` reaches any agent whose domain list includes `healthcare`.

```toml
[overlays.analyst]
domains = ["healthcare", "analytics", "sql"]

[overlays.hermes]
domains = ["messaging"]
```

Domain merging at startup:
1. Static domains from `aletheia.toml` agent definitions
2. Pack overlay domains (union across all loaded packs)
3. Combined domains stored on the agent's config

### High-impact overlay powers

Three overlay fields change more than routing, so they are inert unless the operator opts in via `[packOverlays]` in `aletheia.toml`:

| Field | Effect | Opt-in switch |
|-------|--------|---------------|
| `model` | Overrides the agent's primary model | `allowModelOverrides` |
| `agency` | Overrides the agency level (`unrestricted` = 10000 iterations, `standard`, `restricted` = 50) | `allowAgencyOverrides` |
| `system_prompt_additions` | Injects durable, non-truncatable prompt text into the agent's bootstrap | `allowPromptAdditions` |

```toml
[packOverlays]
allowModelOverrides = false
allowAgencyOverrides = false
allowPromptAdditions = false
maxPromptAdditionBytes = 4096
```

Without the opt-in, declared powers are stripped at pack load; the pack's [health](#pack-health) record lists exactly what was dropped (degraded) or applied (info note). With the opt-in, prompt additions are additionally capped at `maxPromptAdditionBytes` per agent — additions past the cap are dropped whole, never truncated mid-string. `agency` values are validated at load; an unknown level fails the manifest.

## How it works

### Bootstrap injection
Context entries load into `PackSection` values, filter by agent ID and domain tags, convert to `BootstrapSection` values, and merge into the bootstrap assembler alongside workspace files (SOUL.md, USER.md, etc.). Pack sections participate in the same priority sorting and token budget as workspace files.

### Tool registration
Tool definitions are validated (command exists, path is safe, schema parses), converted to `ToolDef` values with category `Domain`, and registered in the shared `ToolRegistry` before agents spawn. Invalid tools are skipped with warnings.

### Domain resolution
At spawn time, the manager calls `sections_for_agent_or_domains(agent_id, domains)` on each loaded pack. A section matches if its `agents` list is empty, contains the agent ID, or contains any of the agent's domain tags.

## How to create a custom pack

1. **Create the pack directory** anywhere on the filesystem (e.g., `instance/packs/my-pack/`).

2. **Write `pack.toml`** with at minimum `name` and `version`:

   ```toml
   name = "my-pack"
   version = "1.0"
   description = "Context and tools for my domain"
   ```

3. **Add context files** under a subdirectory (conventionally `context/`):

   ```text
   my-pack/
     pack.toml
     context/
       DOMAIN_KNOWLEDGE.md
   ```

4. **Reference them in `pack.toml`**:

   ```toml
   [[context]]
   path = "context/DOMAIN_KNOWLEDGE.md"
   priority = "important"
   ```

5. **Register the pack** in `instance/config/aletheia.toml`. Relative paths resolve from the instance root:

   ```toml
   packs = ["packs/my-pack"]
   ```

6. **Restart Aletheia**. The startup log will show `domain pack loaded` for each valid pack.

### Adding a tool

1. Write an executable script under `tools/`:

   ```bash
   #!/usr/bin/env bash
   # Reads JSON from stdin, writes result to stdout.
   # DATABASE_URL comes from the daemon environment, declared in pack.toml.
   INPUT=$(cat)
   QUERY=$(echo "$INPUT" | jq -r '.sql')
   psql "$DATABASE_URL" -c "$QUERY"
   ```

2. Make it executable: `chmod +x tools/query.sh`

3. Declare it in `pack.toml`, including the environment the script needs:

   ```toml
   [[tools]]
   name = "run_query"
   description = "Execute a read-only SQL query"
   command = "tools/query.sh"
   timeout = 30000
   env = ["DATABASE_URL"]

   [tools.input_schema]
   required = ["sql"]

   [tools.input_schema.properties.sql]
   type = "string"
   description = "SQL SELECT statement to execute"
   ```

   Without the `env` declaration the script would run with a cleared environment and `$DATABASE_URL` would be empty — see [Environment and secrets](#environment-and-secrets).

## Filtering to specific agents

Use the `agents` field on context entries and the `overlays` table to target content:

```toml
# Only agent "analyst" sees this section
[[context]]
path = "context/CLINICAL_GUIDELINES.md"
agents = ["analyst"]

# Or target by domain tag  -  any agent with "healthcare" domain receives it
[[context]]
path = "context/ICD_CODES.md"
agents = ["healthcare"]

# Assign the domain tag to analyst via overlay
[overlays.analyst]
domains = ["healthcare"]
```

## Pack resolution order

Packs are loaded in the order they appear in the `packs` config list. When multiple packs match an agent:

- **Context sections**: all matching sections from all packs are included (additive)
- **Tools**: tool names must be unique across all packs. The first registration wins; a later duplicate is skipped with a warning, and the pack that declared it is marked degraded in its [pack health](#pack-health) record
- **Domain overlays**: merged (union) across all packs for each agent

Packs compose additively and do not override or shadow each other.

## Pack health

Every configured pack gets a structured health record (`thesauros::health::PackHealth`) with one of three states:

| Status | Meaning |
|--------|---------|
| `active` | Manifest, all context files, and all tools loaded cleanly |
| `degraded` | Pack is active, but something declared was skipped or failed: a missing optional context file, a tool that failed validation or registration (including a duplicate name), or a dropped overlay power |
| `failed` | Pack is not active at all: the manifest was unreadable/invalid, or a `priority = "required"` context file could not be read |

The startup log prints a `domain pack health` summary line with per-status counts, followed by one warning per recorded issue naming the pack, component, and reason. The structured report is available in-process via `NousManager::pack_report()` for control-plane surfaces.

## See also

- `instance.example/packs/starter/`: minimal working example
- `docs/CONFIGURATION.md`: full `aletheia.toml` reference
- `crates/thesauros/`: pack loader source
