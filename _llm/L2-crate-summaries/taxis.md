# taxis

**Purpose:** Configuration cascade (TOML → env vars), path resolution, and oikos directory structure for the Aletheia runtime.

## Key types

| Type | Purpose |
|------|---------|
| `AletheiaConfig` | Root config struct: agents, gateway, channels, embedding, data, packs, maintenance |
| `Oikos` | Resolved instance paths: root, data, config, logs, nous, shared, theke |
| `NousDefinition` | Per-agent config: model, agency level, tools, limits, domains |
| `CascadeEntry` | Resolved file with path, tier (Nous/Shared/Theke), and filename |
| `ResolvedNousConfig` | Merged agent config after applying defaults and overrides |

## Public API surface

- `taxis::config` - `AletheiaConfig` and all nested config types; `AletheiaConfig::load()` entry point
- `taxis::oikos` - `Oikos` path resolver for instance directory layout
- `taxis::cascade` - Three-tier file discovery (nous/{id}/ → shared/ → theke/)

## When to look here

- When adding a new config field (add to `AletheiaConfig` or a nested struct in `src/config.rs`)
- When resolving instance-relative paths or navigating the oikos directory hierarchy
