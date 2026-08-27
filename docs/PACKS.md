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

- unknown keys at the manifest or any nested context, tool, schema, property, or overlay level reject the manifest; misspelled security policy never falls back to an inherited default
- `name` is 1–64 ASCII alphanumeric/hyphen characters; `version` is non-empty
- every context `path` uses portable relative syntax inside the pack root (no absolute or `..` components)
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
| `name` | string | required | Tool name (alphanumeric, hyphens, underscores) |
| `description` | string | required | Short description sent to the LLM |
| `command` | string | required | Path to script, relative to pack root |
| `timeout` | int | `30000` | Execution timeout in **milliseconds** |
| `groups` | list | `["command"]` | Tool gating groups: `read`, `edit`, `command`, `mcp`, `spawn_subtask`, `plan`, `verify` |
| `tags` | list | `["execute"]` | Operational tags: `recon`, `edit`, `verify`, `fetch`, `spawn`, `plan`, `execute`, `format` |
| `reversibility` | string | `irreversible` | One of `fully_reversible`, `reversible`, `partially_reversible`, `irreversible` |
| `egress` | string | `inherit` | Network egress intent: `inherit` (deployment sandbox policy applies) or `none` (deny outbound network for this tool) |
| `platforms` | list | `["unix"]` | Host platforms the tool supports: `linux`, `macos`, `unix`. A tool whose list excludes the current host is skipped at registration and the pack is marked degraded |
| `input_schema` | object | none | JSON Schema for input parameters |

Input schema properties support types: `string`, `number`, `integer`, `boolean`, `array`, `object`. Each property has a `description` field and optional `enum` and `default` values.

### Tool execution flow

1. LLM emits a `tool_use` block with JSON arguments
2. Runtime serializes arguments to JSON and pipes to the command's stdin
3. Command writes result to stdout (text or JSON)
4. Runtime captures stdout as the tool result
5. Stderr handling is operator-only: content is never copied into the model-visible result or diagnostics. The operator log receives stable warning metadata (tool, exit code, byte count), while arbitrary subprocess bytes are discarded instead of relying on pattern redaction
6. Output is truncated at 50KB

Before every spawn attempt — including each `ETXTBSY` retry — the runtime rechecks the command's registration-time filesystem identity. Execution is still path-based: a mutation after that check but before the kernel resolves `exec` remains a narrower race whose full removal requires descriptor-based execution (`fexecve`/`execveat`).

Diagnostics also record the exit code, wall-clock duration, and — when the sandbox itself refuses to start the command — the stable `sandbox_setup_failed` category, distinguishing "sandbox refused" from "command failed" without copying OS or policy detail across the model boundary.

### Security

- The configured pack root is canonicalized once at load admission. Manifest/context reads, command validation, subprocess cwd, and sandbox read grants all retain that same authority path, so retargeting a configured symlink later cannot redirect the grant. Renaming or mount-replacing the canonical hierarchy itself remains a filesystem-owner boundary; eliminating that race requires descriptor-pinned cwd and sandbox rules
- Command paths are resolved relative to that canonical pack root and canonicalized
- Paths that resolve outside the pack root are rejected (no traversal)
- No shell interpolation: commands receive input only via stdin
- Tools are registered with category `Domain` in the tool registry
- The subprocess environment is cleared except for Organon's fixed safe allowlist (`PATH`, `HOME`, `TERM`, ...). Pack manifests cannot request daemon environment variables
- Pack manifests cannot add write grants. The reserved `env` and `write_paths` fields reject every non-empty declaration until an operator-owned per-pack/per-tool policy can be intersected with the request
- `egress = "none"` can only tighten the deployment's network policy, and the tool is refused when that denial cannot be enforced
- Loaded packs expose read-only accessors; their policy-admitted manifest, context, identity, and root cannot be constructed or mutated by downstream crates to bypass loader policy

### Platform support

Pack tools are shell scripts executed directly via their shebang line, so they are Unix-first by default. The `platforms` field makes a tool's support explicit:

- **Linux**: when the deployment enables an enforcing sandbox and the host supports the full Landlock ABI v5 filesystem-rights baseline plus seccomp, filesystem, syscall, resource, and egress restrictions can all be active. Registration fails when an enforcing host cannot provide every baseline guarantee; a merely partial Landlock ruleset is never admitted as active.
- **macOS and other Unix**: timeout and process-group kill apply, but Landlock, seccomp, egress isolation, and resource limits are unavailable. Enforcing mode refuses pack tools; permissive mode runs with reduced controls and reports each reduced guarantee in pack health.
- **Windows**: pack shell tools are not currently supported. The manifest deliberately has no `windows` platform value until native execution, process-tree cleanup, and CI coverage exist.

The startup health notes are derived from the deployment's actual sandbox configuration and Organon's capability probe. A disabled sandbox is reported explicitly; platform alone is not treated as proof of enforcement.

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

Domain tags are deliberately a global routing namespace, not pack-local authority grants. A context section in any loaded pack opts into that namespace by listing a tag in `agents`, so a domain contributed by one pack may select a tagged section from another pack. Packs are operator-configured together, and domains remain low-impact routing metadata; the separate high-impact powers below still require explicit operator policy.

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

Without the opt-in, declared powers are stripped at pack load; the pack's [health](#pack-health) record lists exactly what was dropped (degraded) or permitted and retained by policy (info note). Policy admission is not an effective runtime diff: agent and provider reconciliation happens later. With the opt-in, prompt additions are additionally capped at `maxPromptAdditionBytes` per pack, per agent — additions past the cap are dropped whole, never truncated mid-string. Pack-controlled bootstrap text is literal: `{{file:...}}` interpolation is rejected in context content and headings, manifest prompt text, overlay additions, tool descriptions, and input-schema prompt strings. Put file content directly in its declared context file; each such file is read through the pack-root containment check and the 512 KiB loader bound. `agency` values are validated at load; an unknown level fails the manifest.

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
   INPUT=$(cat)
   QUERY=$(echo "$INPUT" | jq -r '.sql')
   sqlite3 -readonly data/domain.db "$QUERY"
   ```

2. Make it executable: `chmod +x tools/query.sh`

3. Declare it in `pack.toml`:

   ```toml
   [[tools]]
   name = "run_query"
   description = "Query the pack's bundled read-only domain database"
   command = "tools/query.sh"
   timeout = 30000

   [tools.input_schema]
   required = ["sql"]

   [tools.input_schema.properties.sql]
   type = "string"
   description = "SQL SELECT statement to execute"
   ```

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
- **Model and agency overlays**: among values permitted by operator policy, the last loaded pack targeting the configured agent wins
- **System-prompt additions**: retained additions concatenate in pack configuration order; the byte cap is enforced independently for each pack and agent

Load health records whether policy admitted an overlay. It does not claim that an agent existed, a model resolved, or which competing value became effective at runtime.

## Pack health

Every configured pack gets a structured health record (`thesauros::health::PackHealth`) with one of three states:

| Status | Meaning |
|--------|---------|
| `active` | No degradation has been reported at the current stage. The loader can report this before tool registration; later registration or reconciliation failures are folded into the same record, so the state makes no effectiveness claim |
| `degraded` | Pack loaded, but something declared was skipped or failed: a missing optional context file, a tool that failed validation or registration (including a duplicate name), or a dropped overlay power |
| `failed` | Pack did not load: the manifest was unreadable/invalid, or a `priority = "required"` context file could not be read |

The startup log prints a per-status summary, followed by every recorded issue at its severity with the pack name, configured ordinal, path, component, and reason. The structured report is available in-process via `NousManager::pack_report()` for control-plane surfaces.

## See also

- `instance.example/packs/starter/`: minimal working example
- `docs/CONFIGURATION.md`: full `aletheia.toml` reference
- `crates/thesauros/`: pack loader source
