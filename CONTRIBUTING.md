# Contributing to Aletheia

## Setup

```bash
git clone https://github.com/forkwright/aletheia.git
cd aletheia
```

### Rust
```bash
cargo build && cargo test --workspace && cargo clippy --workspace
```

### TypeScript
```bash
cd infrastructure/runtime && npm install
git config core.hooksPath .githooks
```

## Development

### Local Validation

```bash
# TypeScript — run during development
npm run typecheck && npm run lint:check

# Targeted testing
npx vitest run src/path/to/specific.test.ts
```

**Never run `npm test` locally.** CI handles full test runs. Local full-suite runs are slow and duplicate CI.

### Pre-commit Hook

`.githooks/pre-commit` runs typecheck + lint automatically on staged files. It does not run tests — that's CI's job. Install with `git config core.hooksPath .githooks`.

## Git

### Authorship

All commits use the operator's author identity. Agents are tooling, not contributors.

```
git config user.name "forkwright"
git config user.email "cody.kickertz@pm.me"
```

### Branches

| Type | Pattern | Example |
|------|---------|---------|
| Spec work | `spec<NN>/<description>` | `spec14/dev-workflow` |
| Feature | `feat/<description>` | `feat/gcal-rebuild` |
| Bug fix | `fix/<description>` | `fix/distillation-overflow` |
| Chore/docs | `chore/<description>` | `chore/readme-update` |

Always branch from `main`. Always `git pull --rebase origin main` before pushing. Never commit directly to `main` (except docs-only or trivial config).

### Commits

Conventional commits: `<type>(<scope>): <description>`

Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`
Rules: present tense imperative, first line ≤72 chars, body wraps at 80 chars.

### Squash Policy

**Always squash merge.** Every PR becomes a single commit on `main`. Branch preserves detailed work history.

## Pull Requests

1. Create branch from main
2. Make changes, commit, push
3. Create PR with structured description (use `.github/pull_request_template.md`)
4. CI must pass
5. Squash merge with clean commit message
6. Branch auto-deleted after merge

## Code Standards

Full reference: [docs/STANDARDS.md](docs/STANDARDS.md). Key points:

- Self-documenting code. Comments only for *why*.
- Typed errors — extend `AletheiaError` (TS), use `snafu` (Rust). Never throw strings or bare `Error`.
- No silent catch blocks.
- Greek naming for persistent names per [docs/gnomon.md](docs/gnomon.md).
- Testing: behavior not implementation, descriptive names, same-directory test files.

## Agent Task Dispatch

When dispatching to Claude Code or sub-agents, use the template in [docs/WORKING-AGREEMENT.md](docs/WORKING-AGREEMENT.md).

## Reporting Issues

- **Bugs:** [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
- **Features:** [feature request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Security:** [SECURITY.md](.github/SECURITY.md) — do not open public issues

## License

By contributing, you agree to [AGPL-3.0](LICENSE).
