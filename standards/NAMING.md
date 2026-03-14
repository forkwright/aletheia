# Naming Standards

> Additive to README.md. Read that first. Everything here covers identifier naming, file naming, and project structure.

---

## Code Identifiers

| Context | Convention | Example |
|---------|-----------|---------|
| Types / Traits / Classes | `PascalCase` | `SessionStore`, `MediaProvider` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_TURNS`, `DEFAULT_PORT` |
| Events | `noun:verb` | `turn:before`, `tool:called` |

Function and variable casing is language-specific — see individual language files.

**Universal naming rules:**
- Verb-first for functions: `load_config`, `create_session`, `parse_input`. Drop `get_` prefix on simple getters.
- Boolean variables/columns: `is_` or `has_` prefix.
- Self-documenting over short. `schema_db_path` not `p`. `active_cases` not `df2`.
- If you need a comment to explain what a name means, rename it.

## Gnomon System (Persistent Names)

Module directories, agent identities, subsystems, and major features follow the gnomon naming convention. Names identify **essential natures**, not implementations.

Applies to: modules, crates, agents, subsystems, features that persist across refactors.
Does not apply to: variables, functions, test fixtures, temporary branches.

Process:
1. Identify the essential nature (not the implementation detail)
2. Construct from Greek roots using the prefix-root-suffix system
3. Validate with the layer test (L1 practical → L4 reflexive)
4. Check topology against existing names in the ecosystem
5. If no Greek word fits naturally, the essential nature isn't clear yet — wait

## File & Directory Organization

| Context | Convention | Example |
|---------|-----------|---------|
| Source files | Language convention (see language files) | `session_store.rs`, `SessionStore.cs` |
| Scripts | `kebab-case` | `deploy-worker.sh` |
| Canonical docs | `UPPER_SNAKE.md` | `STANDARDS.md`, `ARCHITECTURE.md` |
| Working docs | `lower-kebab.md` | `planning-notes.md` |
| Directories | `snake_case` | `session_store/`, `test_fixtures/` |
| Timestamped files | `YYYYMMDD_description.ext` | `20260313_export.csv` |

- `snake_case` for directories. No hyphens, no camelCase, no spaces.
- Max 2–3 nesting levels inside any project. Flat > nested.
- No version numbers in filenames — version in file headers or git tags.

## Project Structure

**Group by feature, not by type.** Code that changes together lives together. A feature directory contains its own models, services, routes, and tests. Fall back to layers within a feature when it grows large enough to need internal organization.

| Pattern | When | Example |
|---------|------|---------|
| Feature-first | Default for all projects | `playback/`, `library/`, `auth/` |
| Layers within feature | Feature exceeds ~10 files | `playback/models/`, `playback/services/` |
| Pure layer-based | Small projects (<10 source files) | `models/`, `services/`, `routes/` |

**Predictable top-level directories:**

| Directory | Contents |
|-----------|----------|
| `src/` | All source code. No code at root level. |
| `tests/` | Integration tests (unit tests colocated with source) |
| `scripts/` | Build, deploy, and maintenance scripts |
| `docs/` | Documentation beyond README |
| `config/` | Configuration templates and defaults (not secrets) |

Language-specific layouts (crate structure, package hierarchy) live in the language files.

**Rules:**
- Build artifacts and generated code are gitignored, never committed
- Vendored or third-party code lives in an explicit directory (`vendor/`, `third_party/`), never mixed with project source
- Entry points live in `src/`, not at repository root
- CI configuration in `.github/`, `.gitlab-ci.yml`, or equivalent standard location
