# thesauros

**Purpose:** Domain pack loader: parses `pack.toml` manifests, resolves context files with three-tier priority, and registers pack-declared shell tools into the tool registry.

## Key types

| Type | Purpose |
|------|---------|
| `PackManifest` | Parsed pack.toml: name, version, context entries, tool defs, agent overlays |
| `ContextEntry` | Context file reference: path, priority, agent filter, truncatable flag |
| `LoadedPack` | Fully resolved pack: manifest + sections with file content + root path |
| `PackSection` | Resolved context section: name, content, priority, agent filter, pack name |
| `AgentOverlay` | Per-agent config overlay: model, agency, system prompt additions |

## Public API surface

- `thesauros::manifest` - `PackManifest`, `ContextEntry`, `Priority`, `PackToolDef`
- `thesauros::loader` - `LoadedPack`, `PackSection` for resolved context
- `thesauros::tools` - `ShellToolExecutor`, pack tool registration into `ToolRegistry`

## When to look here

- When building or modifying a domain pack (pack.toml format, context resolution)
- When debugging tool registration from pack-declared shell tool definitions
