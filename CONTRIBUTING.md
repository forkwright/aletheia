# Contributing to Aletheia

## Setup

```bash
git clone https://github.com/forkwright/aletheia.git
cd infrastructure/runtime
npm install
git config core.hooksPath .githooks
```

## Development

```bash
npx tsdown                  # Build
npm test                    # Unit tests
npm run test:coverage       # With coverage thresholds
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
npm run precommit           # All checks
```

Pre-commit hook runs typecheck + lint automatically.

## Pull Requests

- One feature or fix per PR
- Tests for new functionality
- `npm run precommit` passes
- Clear description of what and why
- Reference spec number for spec work (`Spec: 08_memory-continuity.md Phase 4`)

## Code Standards

Full reference: [DEVELOPMENT.md](docs/DEVELOPMENT.md#code-style-conventions). Summary:

**Self-documenting code.** One-line file headers. Inline comments only for *why*, never *what*.

**Typed errors.** All errors extend `AletheiaError`. Error codes in `koina/error-codes.ts`. Never throw strings or bare `Error`. Non-critical ops use `trySafe`/`trySafeAsync` from `koina/safe.ts`.

**Never empty catch.** Every catch logs, rethrows, or returns a meaningful value.

**Naming:** Files `kebab-case`, classes `PascalCase`, functions `camelCase` verb-first, constants `UPPER_SNAKE`, events `noun:verb`.

**TypeScript:** Strict mode, `.js` import extensions, bracket notation for index access.

**Testing:** Behavior not implementation, one assertion per test, descriptive names, same-directory `*.test.ts` files.

## Git

- Descriptive present-tense commits: `fix: prevent orphan messages on pipeline error`
- Always push after commit
- Branches: `spec<N>-<description>` or `fix/<description>`

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](.github/SECURITY.md) — do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE).
