# Contributing

## Setup

```bash
git clone https://github.com/CKickertz/ergon.git
cd infrastructure/runtime
npm install
git config core.hooksPath .githooks
```

## Development

```bash
npx tsdown                  # Build
npm run typecheck           # tsc --noEmit
npm run lint:check          # oxlint
npm run precommit           # Typecheck + lint (pre-commit gate)
```

**Do not run `npm test` locally.** CI handles full test runs.

## Git Conventions

### Authorship

All commits use a single author identity:

```
git config user.name "Cody Kickertz"
git config user.email "cody.kickertz@gmail.com"
```

### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<description>` | `spec14/dev-workflow` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Feature | `feat/<description>` | `feat/gcal-rebuild` |
| Chore/docs | `chore/<description>` | `chore/readme-update` |

Always branch from `main`. Always `git pull --rebase origin main` before pushing.

### Commits

```
<type>: <concise description>

[optional body — what and why, not how]
[optional footer — Spec: NN, Closes #NN]
```

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`

Present tense, imperative mood. First line ≤72 chars.

### Merging

**Always squash merge.** Every PR becomes a single commit on `main`.

## Code Standards

**Self-documenting code.** One-line file headers. Inline comments only for *why*.

**Typed errors.** All errors extend `AletheiaError` with codes from `koina/error-codes.ts`. Never throw strings or bare `Error`.

**No silent catch.** Every catch block must log, rethrow, return a value, or have an inline comment explaining why the error is discarded.

**Naming:** Files `kebab-case`, classes `PascalCase`, functions `camelCase` verb-first, constants `UPPER_SNAKE`. Module names follow the [Gnomon naming system](docs/gnomon.md) — Greek names preserved from upstream.

**Testing:** Behavior not implementation, one assertion per test, descriptive names, same-directory `*.test.ts` files.

## Pull Requests

1. Create branch from main
2. Make changes, commit, push
3. Create PR with description: **What**, **Why**, **Changes**, **Testing**
4. CI must pass
5. Squash merge

## Upstream Sync

This repo is a fork of [forkwright/aletheia](https://github.com/forkwright/aletheia). See [fork-upstream.md](docs/policy/fork-upstream.md) for the boundary between upstream and fork-specific changes.

## License

By contributing, you agree to [AGPL-3.0](LICENSE).
