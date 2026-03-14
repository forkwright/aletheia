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

```yaml
packs:
  - /path/to/my-pack
  - /path/to/another-pack
```

Packs load at startup. Invalid or missing packs log warnings and are skipped (graceful degradation).

## Manifest: pack.toml

```toml
name = "my-domain-pack"
version = "1.0"
description = "Optional description of this pack"

[[context]]
path = "context/BUSINESS_LOGIC.md"
priority = "important"
agents = ["chiron"]

[[context]]
path = "context/GLOSSARY.md"
priority = "flexible"
truncatable = true

[[tools]]
name = "query_database"
description = "Run a read-only SQL query against the data warehouse"
command = "tools/query_database.sh"
timeout = 60000

[tools.input_schema]
required = ["sql"]

[tools.input_schema.properties.sql]
type = "string"
description = "SQL query to execute"

[overlays.chiron]
domains = ["healthcare", "sql"]
```

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

The `agents` field filters which agents receive the section. An empty list means all agents. Values match against both agent IDs (e.g., `chiron`) and domain tags (e.g., `healthcare`).

## Tool definitions

Tools are shell commands exposed to the LLM as callable functions. The runtime pipes JSON to stdin and reads JSON from stdout.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Tool name (alphanumeric + underscores) |
| `description` | string | required | Short description sent to the LLM |
| `command` | string | required | Path to script, relative to pack root |
| `timeout` | int | `30000` | Execution timeout in **milliseconds** |
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

Overlays assign per-agent domain tags. A section tagged `agents = ["healthcare"]` reaches any agent whose domain list includes `healthcare`.

```toml
[overlays.chiron]
domains = ["healthcare", "analytics", "sql"]

[overlays.hermes]
domains = ["messaging"]
```

Domain merging at startup:
1. Static domains from `aletheia.toml` agent definitions
2. Pack overlay domains (union across all loaded packs)
3. Combined domains stored on the agent's config

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

5. **Register the pack** in `instance/config/aletheia.yaml`:

   ```yaml
   packs:
     - instance/packs/my-pack
   ```

6. **Restart Aletheia**. The startup log will show `domain pack loaded` for each valid pack.

### Adding a tool

1. Write an executable script under `tools/`:

   ```bash
   #!/usr/bin/env bash
   # Reads JSON from stdin, writes result to stdout
   INPUT=$(cat)
   QUERY=$(echo "$INPUT" | jq -r '.sql')
   psql "$DATABASE_URL" -c "$QUERY"
   ```

2. Make it executable: `chmod +x tools/query.sh`

3. Declare it in `pack.toml`:

   ```toml
   [[tools]]
   name = "run_query"
   description = "Execute a read-only SQL query"
   command = "tools/query.sh"
   timeout = 30000

   [tools.input_schema]
   required = ["sql"]

   [tools.input_schema.properties.sql]
   type = "string"
   description = "SQL SELECT statement to execute"
   ```

### Filtering to specific agents

Use the `agents` field on context entries and the `overlays` table to target content:

```toml
# Only agent "chiron" sees this section
[[context]]
path = "context/CLINICAL_GUIDELINES.md"
agents = ["chiron"]

# Or target by domain tag — any agent with "healthcare" domain receives it
[[context]]
path = "context/ICD_CODES.md"
agents = ["healthcare"]

# Assign the domain tag to chiron via overlay
[overlays.chiron]
domains = ["healthcare"]
```

## Pack resolution order

Packs are loaded in the order they appear in the `packs` config list. When multiple packs match an agent:

- **Context sections**: all matching sections from all packs are included (additive)
- **Tools**: tool names must be unique across all packs; duplicates are rejected at startup
- **Domain overlays**: merged (union) across all packs for each agent

There is no override or shadowing mechanism; packs compose, they do not replace each other.

## See also

- `instance.example/packs/starter/`: minimal working example
- `docs/CONFIGURATION.md`: full `aletheia.yaml` reference
- `crates/thesauros/`: pack loader source
