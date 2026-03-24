# taxis

Configuration cascade and path resolution: TOML loading, oikos directory structure, env interpolation, hot-reload. 8.2K lines.

## Read first

1. `src/config/mod.rs`: AletheiaConfig root struct and all nested config types
2. `src/oikos.rs`: Oikos instance directory resolver (root, data, config, logs, nous, shared, theke)
3. `src/loader.rs`: Figment-based TOML cascade: defaults -> file -> env vars, with interpolation and decryption
4. `src/cascade.rs`: Three-tier file discovery (nous/{id}/ -> shared/ -> theke/)
5. `src/reload.rs`: Hot-reload classification (restart vs live update) and config diffing

## Key types

| Type | Path | Purpose |
|------|------|---------|
| `AletheiaConfig` | `config/mod.rs` | Root config: agents, gateway, channels, embedding, data, packs, maintenance, sandbox |
| `Oikos` | `oikos.rs` | Resolved instance paths (root, data, config, logs, nous, shared, theke) |
| `Tier` | `cascade.rs` | Which tier a file came from: Nous, Shared, Theke |
| `CascadeEntry` | `cascade.rs` | Resolved file with path, tier, and filename |
| `NousDefinition` | `config/mod.rs` | Per-agent config: model, agency level, tools, limits, domains |
| `ResolvedNousConfig` | `config/resolved.rs` | Merged agent config after applying defaults and overrides |
| `AgentsConfig` | `config/mod.rs` | Agent definitions map, shared defaults, recall settings |
| `GatewayConfig` | `config/mod.rs` | HTTP gateway: port, bind, auth, TLS, CORS, rate limits |
| `ConfigDiff` | `reload.rs` | Set of changed fields between two config versions |
| `PreconditionError` | `preflight.rs` | Collected startup failures (disk space, port, permissions) |
| `WorkspaceSchema` | `workspace_schema.rs` | Validates agent workspace directory structure |
| `ValidationError` | `validate.rs` | Config section validation result |

## Patterns

- **Figment cascade**: Compiled defaults -> TOML file -> `ALETHEIA_*` env vars. Later wins.
- **Env interpolation**: `${VAR:-default}` and `${VAR:?error}` syntax in TOML values, resolved before Figment.
- **Encrypted values**: `enc:` prefix triggers AES-256-GCM decryption using `~/.config/aletheia/primary.key`.
- **Three-tier cascade**: File lookup walks nous/{id}/ -> shared/ -> theke/. Most specific wins.
- **Hot-reload**: Gateway port/bind/TLS/auth require restart. All other config paths are live-reloadable.
- **Preflight checks**: Disk space, port availability, and directory permissions checked before startup.
- **Config redaction**: Secrets stripped before API exposure via `redact` module.

## Common tasks

| Task | Where |
|------|-------|
| Add config section | `src/config/mod.rs` (add field to AletheiaConfig, add nested struct) |
| Add oikos path | `src/oikos.rs` (add method to Oikos) |
| Modify cascade tiers | `src/cascade.rs` (Tier enum, directory walk order) |
| Mark field as restart-required | `src/reload.rs` (RESTART_PREFIXES list) |
| Add preflight check | `src/preflight.rs` (add check to check_preconditions) |
| Add env interpolation syntax | `src/interpolate.rs` |
| Add workspace validation rule | `src/workspace_schema.rs` (WorkspaceSchema) |

## Dependencies

Uses: koina, figment, serde, toml, ring
Used by: nous, pylon, organon, diaporeia, agora, aletheia (binary)
