# Fork/Upstream Relationship

**forkwright/aletheia** (public, framework) → **<your-fork>** (private, operator deployment)

## Boundary

### Always upstream (forkwright/aletheia)
- Runtime source (`infrastructure/runtime/src/`)
- UI source (`ui/src/`)
- Memory sidecar source (`infrastructure/memory/`)
- Shared tooling (`infrastructure/shared/`)
- Documentation (`docs/`)
- CI/CD workflows (`.github/`)
- Schema definitions and migrations

### Always downstream (<your-fork>)
- Agent workspaces (`nous/*/`) — SOUL.md, MEMORY.md, config overrides
- Operator config (`aletheia.json` values — credentials, bindings, agent list)
- Custom commands (`shared/commands/`)
- Custom hooks (`shared/hooks/`)
- Data directories (`data/`)
- Deployment scripts specific to the operator's infrastructure

### Gray zone — case by case
- Plugin manifests — upstream if reusable, downstream if operator-specific
- Spec documents — upstream (design rationale benefits all operators)
- Test fixtures with real data — downstream (privacy)

## Sync Policy

1. **Downstream merges upstream regularly** (at least per minor release)
2. **Downstream never force-pushes upstream changes** — cherry-pick or merge only
3. **Upstream never contains operator-specific config** — use `.gitignore` patterns and config overlays
4. **Conflicts in gray zone**: upstream wins unless operator has a documented reason

## When to Upstream

A change belongs upstream when:
- It fixes a bug any operator would hit
- It adds a capability with no operator-specific assumptions
- It improves documentation or developer experience
- It's a refactor with no behavioral change

A change stays downstream when:
- It references specific credentials, endpoints, or personal data
- It's a workflow or automation specific to one operator's setup
- It customizes agent personality or domain behavior
