# thesauros

Domain pack loader: parses pack.toml manifests, resolves context files, registers pack-declared tools. 2.1K lines.

## Read first

1. `src/manifest.rs`: PackManifest, ContextEntry, PackToolDef, Priority (pack.toml parsing)
2. `src/loader.rs`: LoadedPack, PackSection (context file resolution and agent filtering)
3. `src/tools/mod.rs`: ShellToolExecutor, pack tool registration into ToolRegistry
4. `src/error.rs`: Error variants for manifest parsing, file I/O, and tool registration

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `PackManifest` | `manifest.rs` | Parsed pack.toml: name, version, context entries, tool defs, agent overlays |
| `ContextEntry` | `manifest.rs` | Context file reference: path, priority, agent filter, truncatable flag |
| `Priority` | `manifest.rs` | Bootstrap priority: Required, Important, Flexible, Optional |
| `PackToolDef` | `manifest.rs` | Tool declared in a pack: name, description, command, input schema |
| `AgentOverlay` | `manifest.rs` | Per-agent config overlay (model, agency, system prompt additions) |
| `LoadedPack` | `loader.rs` | Fully resolved pack: manifest, sections with file content read, root path |
| `PackSection` | `loader.rs` | Resolved context section: name, content, priority, agent filter, pack name |
| `PackInputSchema` | `manifest.rs` | Tool input schema: type, properties, required fields |

## Patterns

- **Manifest-driven**: Pack directory contains `pack.toml` plus referenced files. The loader resolves all paths relative to pack root.
- **Agent filtering**: Sections and overlays can target specific agents by ID or domain tags. Empty filter means all agents.
- **Priority cascade**: Context sections declare bootstrap priority (Required > Important > Flexible > Optional) matching the nous bootstrap assembler.
- **Shell tool execution**: Pack tools run as shell scripts with JSON input on stdin, stdout captured as result. ProcessGuard prevents orphan processes.
- **ETXTBSY retry**: Shell executor retries up to 4 times on ETXTBSY (errno 26) to handle races between file writes and exec.

## Common tasks

| Task | Where |
|------|-------|
| Add manifest field | `src/manifest.rs` (PackManifest or nested struct) |
| Add priority level | `src/manifest.rs` (Priority enum) |
| Modify pack loading | `src/loader.rs` (load_pack, resolve context files) |
| Add tool execution mode | `src/tools/mod.rs` (new executor type implementing ToolExecutor) |
| Add overlay type | `src/manifest.rs` (AgentOverlay fields) |

## Dependencies

Uses: koina, organon, indexmap, serde, toml, tokio
Used by: nous, aletheia (binary)
