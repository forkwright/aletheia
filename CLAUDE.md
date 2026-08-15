<!--
scope: aletheia repo conventions (cognition-server crates, recipes, organon tools, theatron UI)
tightens: per-crate CLAUDE.md files under crates/*/ can narrow conventions within their blast radius
-->

# CLAUDE.md

@AGENTS.md

## At a glance

Repo-level conventions for AI coding agents working on Aletheia. Key crates: aletheia, nous, pylon, mneme. Entry point: `crates/aletheia/src/main.rs`.

This file is a thin pointer, not a second copy of [AGENTS.md](AGENTS.md). Build/test/lint commands, key patterns, where to add things, and common mistakes all live there — read it first. What follows is what AGENTS.md does not already cover.

## Depth

Read [docs/GOLDEN-PATH.md](docs/GOLDEN-PATH.md) first for the public desktop-first app workflow, then [docs/HARNESS-LIFECYCLE.md](docs/HARNESS-LIFECYCLE.md) for the canonical nine-stage agent-work loop every crate and surface implements. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/ARCHITECTURE-GUIDE.md](docs/ARCHITECTURE-GUIDE.md) cover the crate/module map and dependency graph.

## Layered context loading

[_llm/README.md](_llm/README.md) defines the L1-L4 on-demand reference system (workspace summary → crate summaries → API index → source) and the task-to-recipe table in `_llm/recipes.toml` — read it there; this file is consumed directly by the agent client and is not itself one of the loaded recipe sections.

## Config

- **Rust crates:** `instance.example/config/aletheia.toml` (TOML cascade: defaults → TOML → env vars)

## Mutation testing

`cargo-mutants` mutates the source and re-runs the tests; a mutation that
passes all tests is a test that does not actually test the code it claims
to cover. Install once per machine, then run against the changed crate
(full workspace takes hours):

```bash
cargo install cargo-mutants
cargo mutants --package <crate> --baseline=skip   # mutate a whole crate
cargo mutants --baseline=skip --in-diff <branch>  # mutate only the diff vs <branch>
```

Baselines for critical paths live under `mutants-out/` (gitignored). Treat
any **missed** mutation as a test gap: either strengthen the existing
assertion or add a new test that catches the mutant. `cargo-mutants` itself
needs nothing beyond `cargo`; see [docs/RELEASING.md](docs/RELEASING.md)'s
maintainer-only release substance-audit appendix for the (separate,
`kanon`-gated) release-time invocation.

## Test data & instance boundary

- All test data MUST use synthetic identities (alice, bob, acme.corp, 192.168.1.100)
- NEVER use real personal information in test fixtures or example data
- Operator-specific config belongs in `instance/` (gitignored), not `shared/` or repo root
- `instance.example/` shows the expected structure for fresh clones
- CI PII scanner rejects commits with personal data patterns (`.github/pii-patterns.txt`)

## Scripts

| Script | Usage |
|--------|-------|
| `scripts/deploy.sh` | Deploy binary to local instance. `--build` to compile, `--restart` to restart service. No flags = full deploy. |
| `scripts/health-monitor.sh` | Health check: service status, API health, token expiry, LLM cost. `--notify` for Signal alerts. |

Systemd timer for health monitoring: `instance.example/services/aletheia-health.{service,timer}` (5-minute interval).

## Git

Conventional commits: `<type>(<scope>): <description>`. Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `ci`, `perf`. Present tense imperative, first line ≤72 chars. Scope is the crate name.

| Branch Type | Pattern | Example |
|-------------|---------|---------|
| Feature | `feat/<description>` | `feat/recall-pipeline` |
| Bug fix | `fix/<description>` | `fix/session-timeout` |
| Docs | `docs/<description>` | `docs/deployment-guide` |
| Refactor | `refactor/<description>` | `refactor/config-cascade` |
| Chore | `chore/<description>` | `chore/update-deps` |

Branch from `main`. Rebase before pushing. Always squash merge.

## Maintainer-only

Everything below assumes access to `forkwright/kanon` (a private repo) and the maintainer's local
`kanon` CLI/audit tooling. None of it is required to open a PR — [AGENTS.md](AGENTS.md)'s `cargo
fmt`/`check`/`clippy`/`nextest` gate is the actual, public requirement (see
[docs/AUTOMATION-PR-GATES.md](docs/AUTOMATION-PR-GATES.md)).

### Standards

Universal: STANDARDS.md in forkwright/kanon's crates/basanos/standards/
Rust: RUST.md in forkwright/kanon's crates/basanos/standards/
Writing: WRITING.md in forkwright/kanon's crates/basanos/standards/
Shell: SHELL.md in forkwright/kanon's crates/basanos/standards/
Naming: GNOMON.md in forkwright/kanon's crates/basanos/standards/, registry at [docs/lexicon.md](docs/lexicon.md)

The public-visible distillation of these — error handling, ID newtypes, async pattern, lint
suppression convention — is in AGENTS.md's Key Patterns; a contributor without kanon access is not
missing a requirement, only the maintainer's own extended style rationale.

<!-- kanon:auto-start -->
## Generated kanon context

- Registry name: `aletheia`
- Forge repo: `forkwright/aletheia`
- Kanon prefix: `al`
- Config source: `workflow/kanon.toml [projects.aletheia]`
- Standards source: `crates/basanos/standards/STANDARDS.md`
- MCP routing catalog: `workflow/AGENTS-mcp-tools.md`

Run `kanon docs sync --check --repo aletheia` to verify this generated
section and `kanon docs sync --apply --repo aletheia` to refresh it.

## Blast zone

- Paths explicitly named by the rendered prompt, role, or template input.

## Acceptance verifier

```bash
kanon gate
```
<!-- kanon:auto-end -->
