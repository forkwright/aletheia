# Domain Packs

A domain pack bundles knowledge, tools, and configuration overlays that extend an Aletheia agent without modifying the core runtime. Packs keep domain-specific content (company IP, schemas, runbooks) separate from generic agent infrastructure.

## Directory Structure

```
my-pack/
  pack.yaml              # Manifest (required)
  context/               # Markdown files injected into bootstrap
    BUSINESS_LOGIC.md
    GLOSSARY.md
  tools/                 # Shell scripts exposed as LLM tools
    query_database.sh
    lookup_schema.py
```

## Configuration

Declare packs in `aletheia.yaml`:

```yaml
packs:
  - /path/to/my-pack
  - /path/to/another-pack
```

Packs load at startup. Invalid or missing packs log warnings and are skipped (graceful degradation).

## Manifest: pack.yaml

```yaml
name: my-domain-pack
version: "1.0"
description: Optional description of this pack

context:
  - path: context/BUSINESS_LOGIC.md
    priority: important
    agents: [chiron]
  - path: context/GLOSSARY.md
    priority: flexible
    truncatable: true

tools:
  - name: query_database
    description: Run a read-only SQL query against the data warehouse
    command: tools/query_database.sh
    timeout: 60
    input_schema:
      properties:
        sql:
          type: string
          description: SQL query to execute
      required: [sql]

overlays:
  chiron:
    domains: [healthcare, sql]
```

## Context Entries

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

The `agents` field filters which agents receive the section. An empty list means all agents. Values match against both agent IDs (e.g., `chiron`) and domain tags (e.g., `healthcare`).

## Tool Definitions

Tools are shell commands exposed to the LLM as callable functions. The runtime pipes JSON to stdin and reads JSON from stdout.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Tool name (alphanumeric + underscores) |
| `description` | string | required | Short description sent to the LLM |
| `command` | string | required | Path to script, relative to pack root |
| `timeout` | int | `30` | Execution timeout in seconds |
| `input_schema` | object | none | JSON Schema for input parameters |

Input schema properties support types: `string`, `number`, `integer`, `boolean`, `array`, `object`. Each property has a `description` field and optional `enum` and `default` values.

### Tool execution flow

1. LLM emits a `tool_use` block with JSON arguments
2. Runtime serializes arguments to JSON and pipes to the command's stdin
3. Command writes result to stdout (text or JSON)
4. Runtime captures stdout as the tool result (stderr is logged, not returned)
5. Output is truncated at 50KB

### Security

- Command paths are resolved relative to the pack root and canonicalized
- Paths that resolve outside the pack root are rejected (no traversal)
- No shell interpolation: commands receive input only via stdin
- Tools are registered with category `Domain` in the tool registry

## Overlays

Overlays assign per-agent domain tags. A section tagged `agents: [healthcare]` reaches any agent whose domain list includes `healthcare`.

```yaml
overlays:
  chiron:
    domains: [healthcare, analytics, sql]
  hermes:
    domains: [messaging]
```

Domain merging at startup:
1. Static domains from `aletheia.yaml` agent definitions
2. Pack overlay domains (union across all loaded packs)
3. Combined domains stored on the agent's config

## How It Works

### Bootstrap injection
Context entries load into `PackSection` values, filter by agent ID and domain tags, convert to `BootstrapSection` values, and merge into the bootstrap assembler alongside workspace files (SOUL.md, USER.md, etc.). Pack sections participate in the same priority sorting and token budget as workspace files.

### Tool registration
Tool definitions are validated (command exists, path is safe, schema parses), converted to `ToolDef` values with category `Domain`, and registered in the shared `ToolRegistry` before agents spawn. Invalid tools are skipped with warnings.

### Domain resolution
At spawn time, the manager calls `sections_for_agent_or_domains(agent_id, domains)` on each loaded pack. A section matches if its `agents` list is empty, contains the agent ID, or contains any of the agent's domain tags.
