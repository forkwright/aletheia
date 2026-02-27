# Staging Environment

## Problem

No staging environment exists. All testing happens in CI (unit/integration) or directly in production. Breaking changes to the runtime, sidecar, or UI are validated only by looking at the live agent.

## Design: Single-Machine Isolation

Use a second set of ports and config files on the same machine.

| Resource | Production | Staging |
|----------|-----------|---------|
| Gateway port | 18789 | 18799 |
| Memory sidecar port | 8230 | 8231 |
| SQLite database | `data/aletheia.db` | `data/staging.db` |
| Qdrant collection | `aletheia` | `aletheia_staging` |
| Neo4j database | `neo4j` | `neo4j` (separate namespace) |
| Config file | `aletheia.json` | `aletheia.staging.json` |
| Workspace root | `nous/` | `nous-staging/` |

## What Staging Enables

- Deploy and smoke-test PRs before they touch the production agent
- Run destructive experiments (schema migrations, memory resets) without risking live data
- Validate the deploy checklist automatically
- Let agents verify their own work end-to-end before requesting merge

## Deployment Flow

```
PR merged to main
  → CI passes
  → Auto-deploy to staging (systemd unit: aletheia-staging)
  → Smoke tests run against staging endpoints
  → Human verifies (or auto-promote after N minutes with no errors)
  → Deploy to production
```

## Smoke Tests

Minimum staging validation before promotion:
1. Gateway responds on `/api/health`
2. At least one agent session can be created
3. A turn completes successfully (tool call + response)
4. Memory sidecar responds on `/health`
5. No error-level log entries in first 60 seconds

## Implementation

1. Create `aletheia.staging.json` with staging ports/paths
2. Create `aletheia-staging.service` systemd unit
3. Add `npm run deploy:staging` script
4. Add smoke test script (`scripts/smoke-test.sh`)
5. Wire into CI as post-merge step
