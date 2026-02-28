# Spec 44: Oikos — Instance Structure and Hierarchical Resolution

**Status:** Draft
**Author:** Syn
**Date:** 2026-02-27
**Spec:** 44
**Origin:** Design discussion (Cody + Syn, 2026-02-27)
**Related:** Spec 36 (Config Taxis — superseded in scope), Spec 37 (Metadata Architecture), Spec 43 (Rust Rewrite)

---

## Problem

### Platform vs. Instance entanglement

The repo conflates two fundamentally different things:

1. **The platform** — source code, specs, UI, CI. Public. Versioned. Identical across deployments.
2. **The instance** — nous identities, memories, config, secrets, session data, signal-cli state. Private. Unique per deployment.

The boundary is enforced by scattered `.gitignore` entries and convention. Two major PRs (#198–203 era) restructured toward separation, but the result is still:
- `nous/` at repo root (gitignored, but positionally ambiguous)
- `shared/` at repo root (partially tracked, partially instance state)
- Credentials in `~/.aletheia/credentials/`, tool config in `shared/config/`, deploy config elsewhere
- USER.md duplicated across every nous workspace, drifting between copies
- Shared research, deliberations, and domain docs squatting in `nous/syn/` because no structural home exists

One bad `git add -A` exposes everything. The gitignore is the only defense.

### No collaborative workspace

Cody and the nous team share work products — research, deliberations, domain references, school work — but there's no declared space for human + nous collaboration. These files accumulate in `nous/syn/` by default because Syn is the catch-all.

The concept of **theke** (θήκη — a place where things are deposited, a repository/storehouse) was used informally early on but never made structural. It should be.

### Flat file resolution

Tools, context, config, and templates are currently resolved through hardcoded paths and per-nous logic. Adding a tool to all nous means editing config. Giving one nous a specific override means special-casing. There's no cascade — no "define at the broadest scope, override where it differs."

### Maintenance overhead

Every new capability (tool, context source, template, hook) requires code changes to wire up. The system should be declarative enough that dropping a file in the right directory is sufficient.

---

## Design

### Principles

1. **One directory, one boundary.** All instance state lives under `instance/`. One gitignore entry. One backup target. One Docker volume. One env var to relocate.

2. **Three-tier hierarchy with cascading resolution.** Theke (human + nous) → shared (nous-only) → nous/{id} (individual). Most specific wins. Same pattern for everything.

3. **Presence is declaration.** A tool YAML file in `instance/theke/tools/` is available to everyone. No registration step, no manifest update. Convention-based discovery.

4. **Parameterize over hardcode.** Model, context sources, tool access, templates, hooks — all declarative config that cascades through the hierarchy. Code changes for capabilities, config changes for policy.

5. **The platform knows the shape; the instance holds the state.** `instance.example/` (tracked) defines the structure. `instance/` (gitignored) holds the live deployment. `aletheia init` copies one to the other.

### The Oikos

**Oikos** (οἶκος) — household, dwelling. The instance is the household where the agents live, where the human collaborates, where state accumulates. The platform is the blueprint; the oikos is the home built from it.

### Directory Structure

```
aletheia/                          # git root — the platform
├── crates/                        # Rust workspace
├── ui/                            # Svelte frontend
├── docs/                          # platform docs, specs, gnomon
├── .github/                       # CI
│
├── instance/                      # ← GITIGNORED — all instance state
│   │
│   ├── theke/                     # Tier 0: human + nous collaborative space
│   │   ├── USER.md               #   Canonical user profile (ONE copy)
│   │   ├── AGENTS.md             #   Team topology
│   │   ├── tools/                #   Tools available to human + all nous
│   │   ├── templates/            #   Shared prompt templates
│   │   ├── research/             #   Shared research products
│   │   ├── deliberations/        #   Multi-agent deliberations
│   │   ├── domains/              #   Domain references (sophia, techne)
│   │   └── projects/             #   Active work products (MBA, etc.)
│   │
│   ├── shared/                   # Tier 1: nous-only shared space
│   │   ├── tools/                #   Tools available to all nous (not human-facing)
│   │   ├── skills/               #   Extracted skill patterns
│   │   ├── hooks/                #   Global event hooks
│   │   ├── templates/            #   System prompt templates
│   │   ├── calibration/          #   Competence calibration data
│   │   ├── bin/                  #   Shared scripts
│   │   └── coordination/         #   Blackboard persistence, task state
│   │
│   ├── nous/                     # Tier 2: individual nous workspaces
│   │   ├── syn/
│   │   │   ├── SOUL.md           #   Identity
│   │   │   ├── TELOS.md          #   Goals (renamed per Spec 33)
│   │   │   ├── MNEME.md          #   Operational memory (renamed per Spec 33)
│   │   │   ├── IDENTITY.md       #   Creature/vibe/emoji
│   │   │   ├── PROSOCHE.md       #   Attention state
│   │   │   ├── TOOLS.md          #   Tool reference (generated)
│   │   │   ├── CONTEXT.md        #   Runtime context (generated)
│   │   │   ├── tools/            #   Nous-specific tools
│   │   │   ├── hooks/            #   Nous-specific hooks
│   │   │   ├── memory/           #   Daily memory files
│   │   │   ├── templates/        #   Nous-specific templates
│   │   │   └── workspace/        #   Scratch space
│   │   ├── demiurge/
│   │   ├── syl/
│   │   └── akron/
│   │
│   ├── config/                   # Deployment configuration (all YAML)
│   │   ├── aletheia.yaml         #   Main runtime config
│   │   ├── credentials/          #   OAuth tokens, API keys, session key
│   │   ├── prosoche.yaml         #   Attention daemon config
│   │   └── bindings.yaml         #   Channel → nous bindings
│   │
│   ├── data/                     # Runtime data stores
│   │   ├── sessions.db           #   SQLite (WAL mode)
│   │   ├── planning.db           #   Dianoia state (or same DB, separate concern)
│   │   ├── qdrant/               #   Qdrant persistent storage (Docker mount)
│   │   └── neo4j/                #   Neo4j persistent storage (Docker mount)
│   │
│   ├── signal/                   # signal-cli data directory
│   │
│   └── logs/                     # Runtime logs (if not journald)
│
└── instance.example/              # ← TRACKED — scaffold template
    ├── theke/
    │   ├── USER.md.example
    │   └── AGENTS.md.example
    ├── shared/
    │   ├── tools/
    │   └── templates/
    ├── nous/
    │   └── _template/            #   Template for `aletheia add-nous`
    │       ├── SOUL.md
    │       ├── TELOS.md
    │       ├── MNEME.md
    │       ├── IDENTITY.md
    │       └── PROSOCHE.md
    └── config/
        ├── aletheia.yaml.example
        └── prosoche.yaml.example
```

### Three-Tier Cascade

Resolution order for any resource: **nous/{id} → shared → theke**

Most specific tier wins for single-value lookups (config overrides). All tiers merge for collection lookups (tools, templates, hooks).

```rust
// taxis/src/oikos.rs

/// The oikos root — all instance state
pub fn instance_root() -> PathBuf {
    env::var("ALETHEIA_INSTANCE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./instance"))
}

/// Tier 0: human + nous collaborative space
pub fn theke() -> PathBuf { instance_root().join("theke") }

/// Tier 1: nous-only shared
pub fn shared() -> PathBuf { instance_root().join("shared") }

/// Tier 2: individual nous workspace
pub fn nous(id: &str) -> PathBuf { instance_root().join("nous").join(id) }

/// Config directory
pub fn config() -> PathBuf { instance_root().join("config") }

/// Data directory
pub fn data() -> PathBuf { instance_root().join("data") }

/// Resolve a resource path across all tiers (most specific first)
pub fn resolve(nous_id: &str, resource: &str) -> Vec<PathBuf> {
    [nous(nous_id), shared(), theke()]
        .into_iter()
        .map(|base| base.join(resource))
        .filter(|p| p.exists())
        .collect()
}

/// Resolve single resource — most specific tier wins
pub fn resolve_one(nous_id: &str, resource: &str) -> Option<PathBuf> {
    resolve(nous_id, resource).into_iter().next()
}

/// Resolve and collect all matching resources (for glob/directory merges)
pub fn resolve_all(nous_id: &str, resource: &str) -> Vec<PathBuf> {
    resolve(nous_id, resource)
}
```

### Hierarchical Resolution: Concrete Examples

**Tools:**
```
instance/theke/tools/gcal.yaml          → human + all nous
instance/shared/tools/distill.yaml      → all nous
instance/nous/syn/tools/pplx.yaml       → only syn
```

Runtime asks "what tools does syn have?" → walks all three tiers, collects all YAML files, deduplicates by tool name (most specific wins on conflict).

**Context assembly:**
```
instance/theke/USER.md                  → loaded for every nous
instance/theke/AGENTS.md                → loaded for every nous
instance/nous/syn/SOUL.md               → loaded for syn only
instance/nous/syn/MNEME.md              → loaded for syn only
```

No more hardcoded `assemble-context` scripts. The bootstrap phase walks the cascade:
1. Load all `*.md` from theke that match the context schema
2. Load all `*.md` from shared that match
3. Load all `*.md` from nous/{id} that match
4. Apply token budget constraints (priority order from config)

**Config cascade:**
```yaml
# instance/shared/defaults.yaml — all nous inherit
model: anthropic/claude-sonnet-4-20250514
context_window: 200000
pipeline:
  exec_timeout: 120s
  max_tool_rounds: 25

# instance/nous/syn/overrides.yaml — syn-specific
model: anthropic/claude-opus-4-6
pipeline:
  max_tool_rounds: 50
```

Deep merge with most-specific-wins. Syn gets Opus + 50 rounds. Everyone else gets Sonnet + 25 rounds. No per-agent config blocks in `aletheia.yaml` — just files in the right place.

**Hooks:**
```
instance/shared/hooks/on-turn-complete.yaml     → fires for all nous
instance/nous/syn/hooks/on-turn-complete.yaml    → fires for syn only (overrides? appends?)
```

**Decision:** Nous-specific hooks supplement shared hooks (both fire). Shared fires first, then nous-specific. Explicit `override: true` in the nous-specific hook replaces instead.

**Templates:**
```
instance/shared/templates/system-prompt.md      → default system prompt template
instance/nous/demiurge/templates/system-prompt.md → demiurge's custom system prompt
```

Most-specific wins for templates (they're complete documents, not collections).

### Defaults YAML

Each tier can contain a `defaults.yaml` that sets baseline values for that scope:

```yaml
# instance/shared/defaults.yaml
model: anthropic/claude-sonnet-4-20250514
context_window: 200000
pipeline:
  exec_timeout: 120s
  max_tool_rounds: 25
  parallel_tool_calls: true
context:
  always:
    - theke://USER.md
    - theke://AGENTS.md
  on_start:
    - self://SOUL.md
    - self://MNEME.md
    - self://TELOS.md
    - self://IDENTITY.md
tools:
  include: "**/*.yaml"   # convention: all YAML in tools/ directory
  exclude: []
hooks:
  mode: supplement       # nous-specific hooks ADD to shared, don't replace
```

```yaml
# instance/nous/syn/overrides.yaml
model: anthropic/claude-opus-4-6
context:
  on_start:
    - +self://PROSOCHE.md   # APPEND to shared defaults (+ prefix)
tools:
  include:
    - +pplx.yaml            # APPEND (adds to shared tools, doesn't replace)
```

The `self://` prefix resolves to the current nous's workspace. The `theke://` prefix resolves to theke. The `shared://` prefix resolves to shared. No prefix = relative to current tier. The `+` prefix on array values means append to the parent tier's array rather than replace it.

### Safety: Belt and Suspenders

**Primary:** `instance/` in `.gitignore`.

**Secondary:** Pre-commit hook rejects any staged file under `instance/`:
```bash
#!/bin/bash
# .githooks/pre-commit
if git diff --cached --name-only | grep -q '^instance/'; then
  echo "ERROR: Attempting to commit instance files. These must never be tracked."
  echo "Files:"
  git diff --cached --name-only | grep '^instance/'
  exit 1
fi
```

**Tertiary:** CI check — fail the build if any file under `instance/` exists in the tree.

**Quaternary:** `aletheia doctor` validates that `instance/` is gitignored and no tracked files exist under it.

### Migration from Current Layout

| Current Location | New Location | Notes |
|-----------------|-------------|-------|
| `nous/syn/` | `instance/nous/syn/` | Entire workspace moves |
| `nous/_shared/` | `instance/shared/` | Rename |
| `nous/_example/` | `instance.example/nous/_template/` | Tracked scaffold |
| `shared/config/` | `instance/shared/` + `instance/config/` | Split by purpose |
| `shared/bin/` | `instance/shared/bin/` | Moves |
| `shared/skills/` | `instance/shared/skills/` | Moves |
| `shared/hooks/` | `instance/shared/hooks/` | Moves |
| `shared/tools/` | `instance/shared/tools/` | Moves |
| `~/.aletheia/` | `instance/config/` + `instance/data/` | Consolidated |
| `nous/syn/deliberations/` | `instance/theke/deliberations/` | Shared, not syn-owned |
| `nous/syn/domains/` | `instance/theke/domains/` | Shared, not syn-owned |
| `nous/syn/mba/` | `instance/theke/projects/mba/` | Shared work product |
| `nous/syn/research/` | `instance/theke/research/` | Shared, not syn-owned |
| Multiple `USER.md` copies | `instance/theke/USER.md` | ONE canonical copy |

### Scaffold and Init

```bash
# New deployment
aletheia init
# → Copies instance.example/ → instance/
# → Prompts for secrets (Anthropic key, Signal number, etc.)
# → Generates session key, memory token
# → Creates systemd unit if desired

# Add a nous
aletheia add-nous demiurge
# → Copies instance.example/nous/_template/ → instance/nous/demiurge/
# → Prompts for SOUL.md customization
# → Registers in config
```

### Docker / Deployment

```yaml
# docker-compose.yaml
services:
  aletheia:
    image: aletheia:latest
    volumes:
      - ./instance:/app/instance    # ONE volume, all state
    environment:
      - ALETHEIA_INSTANCE=/app/instance
```

Backup: `tar czf backup-$(date +%Y%m%d).tar.gz instance/`
Restore: `tar xzf backup.tar.gz`

### Accessibility (Future)

The theke directory is the natural unit for cross-device sync:
- **Syncthing** between server and Metis — bidirectional, real-time
- **Git sub-repo** — version-controlled separately from the platform
- **NAS mount** — read from any device on the network
- **Webchat file browser** — scope to theke only (safe boundary for UI exposure)

The structural decision now doesn't constrain which mechanism is chosen later. Theke is one clean directory — whatever sync tool, it targets one path.

---

## Phases

### Phase 1: Structure and Paths

**Scope:** Create the oikos directory structure, implement path resolution in taxis, add safety mechanisms.

**Changes:**

- `instance.example/` — Create scaffold template with full directory tree and example files
- `.gitignore` — Add `instance/` entry
- `.githooks/pre-commit` — Add instance-file rejection hook
- `crates/taxis/src/oikos.rs` — Implement path resolution functions (instance_root, theke, shared, nous, config, data, resolve, resolve_one, resolve_all)
- `crates/taxis/src/paths.rs` — Update all existing path functions to delegate to oikos
- `crates/taxis/src/lib.rs` — Export oikos module
- `aletheia doctor` — Add instance/gitignore validation check

**Acceptance Criteria:**
- [ ] `instance.example/` contains complete scaffold with all directories and example files
- [ ] `instance/` is gitignored with pre-commit hook protection
- [ ] `oikos::resolve()` correctly walks three tiers with most-specific-first ordering
- [ ] `oikos::instance_root()` respects `ALETHEIA_INSTANCE` env var
- [ ] All existing path references in taxis route through oikos functions
- [ ] `aletheia doctor` validates oikos structure
- [ ] CI check confirms no tracked files under `instance/`

**Testing:**
- Unit tests for resolve/resolve_one/resolve_all with fixtures at each tier
- Test env var override for instance root
- Test missing tiers (sparse oikos — not every nous needs every directory)
- Integration test: `aletheia init` produces valid oikos from template

### Phase 2: Hierarchical Tool Resolution

**Scope:** Tools discovered by convention from all three tiers. No registration, no code changes to add a tool.

**Changes:**

- `crates/organon/src/discovery.rs` — Implement tool discovery that walks `oikos::resolve_all(nous_id, "tools")` and loads all YAML tool definitions
- `crates/organon/src/registry.rs` — Update registry to accept discovered tools, handle deduplication (most-specific-wins by tool name)
- Tool YAML schema — Define the declarative tool definition format
- Move existing built-in tool configs to appropriate tiers in `instance.example/`

**Acceptance Criteria:**
- [ ] Dropping a YAML file in any tier's `tools/` directory makes it available to the appropriate scope
- [ ] Tool at nous tier overrides same-named tool at shared tier
- [ ] Removing a YAML file removes the tool (no stale registry entries)
- [ ] Built-in tools work through the same discovery mechanism
- [ ] Hot-reload on file change (or at minimum, reload on nous restart)

**Testing:**
- Tool present at theke → available to all nous
- Tool present at nous/syn → available only to syn
- Override: tool "gcal" at nous/syn overrides theke/tools/gcal
- No YAML files → no custom tools (built-ins still work through code)

### Phase 3: Hierarchical Context Assembly

**Scope:** Context bootstrap reads from the cascade. No more hardcoded file lists.

**Changes:**

- `crates/nous/src/bootstrap.rs` — Rewrite context assembly to use `oikos::resolve` for all workspace files
- `defaults.yaml` schema — Define `context.always` and `context.on_start` fields with `theke://`, `shared://`, `self://` prefix resolution
- Token budget system — Apply priority ordering from config when context exceeds window
- Deprecate `assemble-context` shell script (replaced by in-process resolution)
- Deprecate `compile-context` (CONTEXT.md and TOOLS.md become generated views, not manually compiled)

**Acceptance Criteria:**
- [ ] USER.md loaded from `theke/USER.md` — no per-nous copies
- [ ] AGENTS.md loaded from `theke/AGENTS.md` — no per-nous copies
- [ ] SOUL.md, MNEME.md, TELOS.md loaded from `nous/{id}/`
- [ ] Additional context sources configurable via `defaults.yaml` + `overrides.yaml`
- [ ] Token budget respects priority ordering
- [ ] `self://`, `theke://`, `shared://` prefixes resolve correctly

**Testing:**
- Context assembly for syn includes theke/USER.md + nous/syn/SOUL.md
- Context assembly for demiurge includes theke/USER.md + nous/demiurge/SOUL.md (same USER.md)
- Override in nous-level overrides.yaml adds additional context sources
- Token budget truncation respects priority

### Phase 4: Config Cascade

**Scope:** Runtime configuration merges across tiers. One `defaults.yaml` at shared, per-nous `overrides.yaml`.

**Changes:**

- `crates/taxis/src/cascade.rs` — Implement deep-merge of YAML configs across tiers
- `crates/taxis/src/loader.rs` — Update config loading to walk oikos cascade
- Define merge semantics: scalars (last wins), arrays (replace by default, `+` prefix to append), maps (recursive merge)
- `instance.example/shared/defaults.yaml` — Template with all configurable fields documented
- `instance.example/nous/_template/overrides.yaml` — Template showing override pattern

**Acceptance Criteria:**
- [ ] `defaults.yaml` at shared tier provides baseline for all nous
- [ ] `overrides.yaml` at nous tier selectively overrides fields
- [ ] Deep merge handles nested config correctly
- [ ] Array merge semantics are explicit (replace by default, append with `+` prefix convention)
- [ ] Missing override file → defaults used (no error)
- [ ] Invalid override → fast-fail at boot with clear error
- [ ] `aletheia doctor` validates merged config

**Testing:**
- Shared defaults only → valid config
- Shared defaults + nous override → merged correctly
- Conflicting values → nous tier wins
- Nested override (e.g., pipeline.max_tool_rounds only) → other fields inherit
- Invalid YAML → clear error message with file path

### Phase 5: Hooks and Templates

**Scope:** Hooks and templates discovered through the cascade. Supplement mode for hooks, override mode for templates.

**Changes:**

- Hook discovery walks `oikos::resolve_all(nous_id, "hooks")`, collects and merges
- Template discovery walks `oikos::resolve(nous_id, "templates")`, most-specific wins
- Hook merge mode: supplement (default) or override (explicit `override: true` in file)
- Template merge mode: always override (complete documents)

**Acceptance Criteria:**
- [ ] Shared hook fires for all nous
- [ ] Nous-specific hook supplements shared hooks by default
- [ ] Nous-specific hook with `override: true` replaces shared hook of same name
- [ ] Nous-specific template replaces shared template of same name
- [ ] Hot-reload when feasible

**Testing:**
- Shared hook fires for syn and demiurge
- Syn-specific hook fires for syn only, in addition to shared hook
- Override hook replaces shared hook
- Template at nous tier takes precedence over shared tier

### Phase 6: Migration

**Scope:** Move current files from scattered locations into the oikos structure. This is a one-time migration for the existing deployment.

**Changes:**

- Migration script: `aletheia migrate-to-oikos`
  - Moves `nous/*/` → `instance/nous/*/`
  - Moves `shared/` → `instance/shared/`
  - Moves `~/.aletheia/` → `instance/config/` + `instance/data/`
  - Creates `instance/theke/` and populates from syn's shared files
  - Removes duplicate USER.md from all nous workspaces
  - Updates symlinks/references
- Validation: `aletheia doctor` confirms oikos integrity post-migration
- Rollback: migration creates backup tarball first

**Acceptance Criteria:**
- [ ] All instance state consolidated under `instance/`
- [ ] No instance files remain at repo root
- [ ] All nous workspaces functional after migration
- [ ] One canonical USER.md in theke
- [ ] Deliberations, research, domains moved from syn to theke
- [ ] `aletheia doctor` passes
- [ ] Pre-commit hook active

**Testing:**
- Dry-run mode shows what would move without moving it
- Post-migration health check (all nous respond, all tools available)
- Rollback restores previous state

---

## Relationship to Other Specs

### Spec 36 (Config Taxis) — Largely Superseded

Spec 36's "4-Layer Architecture" (Framework / Identity+workspace / Team work / Deployment config) is replaced by the oikos three-tier model. The concepts overlap but oikos is more concrete:

| Spec 36 Layer | Oikos Equivalent |
|---------------|-----------------|
| Framework | The platform (git-tracked repo root) |
| Identity + workspace | `instance/nous/{id}/` |
| Team work | `instance/theke/` (human + nous) + `instance/shared/` (nous-only) |
| Deployment config | `instance/config/` |

Spec 36's SecretRef, exec tool config, deploy pipeline, and sidecar security concerns remain valid and should be absorbed into this spec or retained as implementation details under oikos.

### Spec 37 (Metadata Architecture) — Aligned, Scoped

Spec 37's "declarative over imperative" principle is realized through the oikos cascade. Convention-based discovery, schema-first validation, and configuration cascade are all implemented here. Spec 37 becomes the philosophical backing; oikos is the implementation.

### Spec 43 (Rust Rewrite) — Depends On

The rewrite's `taxis::paths` module implements oikos resolution. Every crate that needs state goes through `taxis::oikos`. The oikos structure must be defined before crate implementation begins.

### Spec 33 (Gnomon Alignment) — Workspace file renames

GOALS.md → TELOS.md and MEMORY.md → MNEME.md (Spec 33 Phase 2) are assumed complete in the oikos layout. If not yet done, they happen during Phase 6 migration.

---

## Decisions (Resolved 2026-02-27)

1. **Hook merge semantics.** Supplement by default — shared hooks fire first, then nous-specific. Explicit `override: true` in a nous-specific hook replaces the shared hook of the same name.

2. **Generated files (CONTEXT.md, TOOLS.md).** Written to disk. Debugging value wins — `cat` from terminal, inspect without runtime. Runtime regenerates on boot and on relevant changes.

3. **Config format.** YAML (`aletheia.yaml`). Human-friendly, supports comments, natural for the cascade pattern. Clean break during the rewrite — no migration of existing JSON needed since the rewrite reimplements config loading.

4. **Neo4j data location.** Mount under `instance/data/neo4j/`. Unified backup — one `tar` captures everything including graph state.

5. **Skills directory.** `instance/shared/skills/`. These are generated per-deployment (extracted from this instance's sessions), not platform-level reference data.

6. **Signal-cli data.** Move to `instance/signal/`. Unified backup captures Signal registration and device state.

7. **Array merge behavior.** Replace by default. Prefix array values with `+` to append to the parent tier's array instead of replacing it. Explicit and unambiguous.

8. **Path syntax.** URI-style prefixes: `self://`, `theke://`, `shared://`. Clean, unambiguous, extensible. `self://` resolves to the current nous workspace. No prefix = relative to current tier.

9. **Migration timing.** Migrate now (pre-rewrite). The structural clarity helps immediately and validates the design. TS runtime path updates are mechanical.

## Open Questions

1. **Theke sync mechanism.** Deferred. Syncthing vs. git sub-repo vs. NAS mount. The directory structure is sync-agnostic — decide when cross-device access becomes a priority.

---

## References

- Design discussion: Cody + Syn, 2026-02-27 (origin of theke concept and hierarchy)
- Spec 36: Config Taxis (predecessor, partially superseded)
- Spec 37: Metadata Architecture (philosophical alignment)
- Spec 43: Rust Rewrite (implementation vehicle)
- Spec 33: Gnomon Alignment (workspace file renames)
- PRs #198–203: Prior work on platform/instance separation
