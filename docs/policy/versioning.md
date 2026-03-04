# Versioning and Breaking Change Policy

## Version Scheme

Semantic versioning with pre-1.0 interpretation:

| Version | Meaning |
|---------|---------|
| `0.MINOR.PATCH` | Pre-stable. Breaking changes allowed in MINOR bumps with documented migration |
| `1.0.0` | Stable public API. Breaking changes require MAJOR bump |

**Current:** `0.10.0` (pre-stable)

The canonical version lives in `Cargo.toml` under `[workspace.package].version`.
All crates inherit it via `version.workspace = true`. The release-please manifest
(`.release-please-manifest.json`) tracks the same value.

## What Constitutes a Breaking Change

### Breaking (requires MINOR bump)
- Removing or renaming a config key in `aletheia.json`
- Changing database schema without automatic migration
- Removing or renaming a tool available to agents
- Changing API endpoint signatures (request/response shape)
- Removing or renaming a CLI command
- Changing the plugin/hook interface contract

### Non-breaking (PATCH bump)
- Adding new config keys with defaults
- Adding new API endpoints
- Adding new tools
- Adding new database columns with defaults
- Bug fixes
- Performance improvements
- Documentation changes

## Migration Path

Every breaking change includes:
1. **Migration guide** in the release notes
2. **Automated migration** where possible (schema migrations, config transformers)
3. **Deprecation period** of at least one minor release for removals (add deprecation warning in N, remove in N+1)

## Release Process

1. Merge to `main` with conventional commit messages
2. release-please opens a version-bump PR (updates `CHANGELOG.md`, manifest, `Cargo.toml`)
3. Merge the release PR to create a git tag
4. CI builds cross-platform binaries and attaches them to the GitHub Release
5. Notify downstream operators if breaking

Manual fallback: `scripts/bump-version.sh <version>`, then tag and push.

See `docs/RELEASING.md` for the full process.

## Changelog Format

```markdown
## [0.11.0] — 2026-MM-DD

### Breaking
- Renamed `agents.list[].pipeline` to `agents.list[].config` — run `aletheia migrate`

### Added
- New tool: `plan_discuss` for phase-level discussion flow

### Fixed
- Memory recall diversity regression from v0.10.3

### Changed
- Default context window reduced from 200k to 128k for cost optimization
```
