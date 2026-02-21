# Claude Code Assignment: Test Coverage Expansion

## Context
We have 107 test files for 143 source files. 44 source files have zero tests. 
The 10 manager.test.ts failures are already fixed (PR #92 merged).
All existing tests pass. Branch from `main`.

## Branch
`test/coverage-expansion`

## Rules
1. **NEVER run the full vitest suite.** Always target: `npx vitest run src/path/to/file.test.ts`
2. **Follow existing test patterns.** Look at neighboring `.test.ts` files for mock patterns and conventions.
3. **Every test file must pass independently** before moving to the next.
4. **Use `vi.mock()` for external dependencies** (filesystem, network, Anthropic API, Signal CLI, etc.)
5. **Test behavior, not implementation.** Focus on: input validation, error paths, edge cases, return values.
6. **No snapshot tests.** They're brittle. Assert specific values.
7. **Commit after each tier** with a descriptive message.
8. **Do NOT modify source files.** Test-only changes. If a source file is untestable without changes, note it and skip.
9. **Do NOT touch `manager.test.ts` or `manager-streaming.test.ts`** — already fixed.

## Tier 2: Pipeline Stages (HIGHEST PRIORITY — core system logic)

These are the brain of the system. Zero tests currently.

| File | Lines | Test Focus |
|------|-------|------------|
| `src/nous/pipeline/stages/execute.ts` | 778 | Tool loop logic, stop conditions, queue drain, plan detection, streaming vs buffered paths, max tool loops |
| `src/nous/pipeline/stages/context.ts` | 249 | Bootstrap assembly, distillation priming injection, recall merging, token budget |
| `src/nous/pipeline/stages/finalize.ts` | 136 | Message persistence, usage recording, fact extraction trigger, working state save |
| `src/nous/pipeline/stages/guard.ts` | 48 | Depth limit check, circuit breaker check, lock acquisition |
| `src/nous/pipeline/stages/history.ts` | 68 | History retrieval, budget calculation, message ordering |
| `src/nous/pipeline/stages/resolve.ts` | 123 | Nous resolution, session creation, route resolution, default fallback |

**Approach:** Each stage is a pure-ish function taking `(context, services)` and returning updated context. Mock `services` (store, router, tools, config). For `execute.ts`, you'll need to mock the Anthropic response cycle — see `manager.test.ts` `makeRouter()` for the pattern.

For `context.ts` specifically, the mock store needs `getDistillationPriming` (returns null or `{ summary, facts }`).

## Tier 3: Auth Stack (security code = must test)

| File | Lines | Test Focus |
|------|-------|------------|
| `src/auth/middleware.ts` | 269 | Token validation, session auth, CORS, rate limiting, public routes bypass |
| `src/auth/sessions.ts` | 212 | Session creation, expiry, refresh, invalidation |
| `src/auth/rbac.ts` | 151 | Role checks, permission resolution, admin override, deny-by-default |
| `src/auth/tokens.ts` | 63 | JWT generation, verification, expiry, malformed input |
| `src/auth/passwords.ts` | 45 | Hash, verify, timing-safe comparison |
| `src/auth/audit-verify.ts` | 89 | Audit log integrity verification |
| `src/auth/retention.ts` | 113 | Data retention policy enforcement, cleanup |
| `src/auth/sanitize.ts` | 12 | Input sanitization (small but security-critical) |
| `src/auth/tls.ts` | 151 | Cert loading, self-signed generation, validation |

**Note:** `audit.ts` already has `audit.test.ts`. Don't duplicate.

## Tier 4: High-Value Singles

| File | Lines | Test Focus |
|------|-------|------------|
| `src/distillation/similarity-pruning.ts` | 125 | Dedup logic, similarity threshold, pruning order |
| `src/nous/interaction-signals.ts` | 119 | Signal extraction from messages, pattern matching |
| `src/organon/built-in/plan-propose.ts` | 118 | Plan validation, step structure, cost calculation |
| `src/organon/skill-learner.ts` | 137 | Skill extraction, dedup, storage |
| `src/organon/workspace-git.ts` | 57 | Git status, commit, push operations |
| `src/organon/mcp-client.ts` | 269 | MCP protocol handling, tool discovery, execution proxy |
| `src/organon/config/sub-agent-roles.ts` | — | Role definition loading, prompt template generation |
| `src/daemon/reflection-cron.ts` | 178 | Cron scheduling, reflection trigger conditions |
| `src/daemon/retention.ts` | 76 | Retention cleanup execution |

## Tier 5: Tool Implementations (thin but verify arg validation)

| File | Lines | Test Focus |
|------|-------|------------|
| `src/organon/built-in/exec.ts` | 85 | Command execution, timeout, output truncation |
| `src/organon/built-in/edit.ts` | 79 | Find/replace, uniqueness check, whitespace handling |
| `src/organon/built-in/read.ts` | — | File read, maxLines, binary detection |
| `src/organon/built-in/write.ts` | — | File write, append mode, directory creation |
| `src/organon/built-in/find.ts` | — | Pattern matching, type filter, depth limit |
| `src/organon/built-in/grep.ts` | — | Regex search, glob filter, result limiting |
| `src/organon/built-in/ls.ts` | — | Directory listing, hidden files, formatting |
| `src/organon/built-in/memory-correct.ts` | — | Memory correction, validation |
| `src/organon/built-in/memory-forget.ts` | — | Memory deletion, confirmation |

**For filesystem tools:** Use `tmp` directories and clean up. Don't test against real workspace.

## Definition of Done
- All new test files pass: `npx vitest run src/path/to/new.test.ts`
- Zero modifications to source files
- Single PR with all tiers, commits grouped by tier
- Final commit message includes count: "test: add X test files covering Y untested source files"

## Existing Test Examples to Reference
- `src/organon/built-in/blackboard.test.ts` — good pattern for tool tests
- `src/nous/manager.test.ts` — mock patterns for store/router/tools
- `src/distillation/pipeline.test.ts` — mock patterns for distillation
- `src/hermeneus/complexity.test.ts` — pure function testing
- `src/koina/pii.test.ts` — thorough input validation testing
