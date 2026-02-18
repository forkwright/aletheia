# Contributing

## Getting Started

1. Fork the repository
2. Clone your fork and set up the dev environment:
   ```bash
   cd infrastructure/runtime
   npm install
   git config core.hooksPath .githooks
   ```
3. Create a branch from `main`

## Development

### Build

```bash
cd infrastructure/runtime
npx tsdown
```

### Test

```bash
npm test                    # Unit tests
npm run test:coverage       # With coverage thresholds
npm run test:integration    # Integration tests (30s timeout)
```

### Lint & Type Check

```bash
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
npm run precommit           # All checks (typecheck + lint + test)
```

### Pre-commit Hook

The hook runs `typecheck` and `lint:check` automatically. Enable it with:

```bash
git config core.hooksPath .githooks
```

## Code Style

- **TypeScript strict mode** - all strict flags enabled, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`
- **File headers** - single-line comment describing the file's purpose. No JSDoc, no dates, no author info.
- **Imports** - use `.js` extensions (NodeNext resolution). Group: node builtins, then local.
- **Naming** - files: `kebab-case.ts`, classes: `PascalCase`, functions: `camelCase`, constants: `UPPER_SNAKE_CASE`
- **Index access** - bracket notation for string-keyed records (`record["key"]`, not `record.key`)
- **Tests** - adjacent to source (`foo.ts` / `foo.test.ts`), vitest with `describe`/`it`

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full style guide and module architecture.

## Pull Requests

- Keep PRs focused - one feature or fix per PR
- Include tests for new functionality
- Ensure `npm run precommit` passes
- Write a clear description of what changed and why
- Reference related issues with `Fixes #123` or `Closes #123`

## Adding Tools

Tools live in `src/organon/built-in/`. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#adding-new-tools) for the full guide.

## Adding Commands

Signal commands (`!command`) are registered in `src/semeion/commands.ts`. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#adding-new-built-in-commands).

## Reporting Issues

- **Bugs** - use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features** - use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security** - see [SECURITY.md](.github/SECURITY.md). Do not open public issues for vulnerabilities.

## License

By contributing, you agree that your contributions will be licensed under the [AGPL-3.0](LICENSE).
