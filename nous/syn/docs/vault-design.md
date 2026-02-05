# Vault Design — Aletheia

## Principle

Two audiences, two layers:

**Vault** (`vault/`) — Cody's workspace. Everything he'd read, edit, write, reference. Obsidian-native. Human-first.

**Nous** (`nous/`) — Agent workspaces. Machine-first. Configs, memory, operating procedures, tools. Agents work here and *publish* human-facing outputs to vault.

The vault is not a mirror of nous. It's the *product* of the system.

---

## Vault Structure

```
vault/                          ← Obsidian root
├── HOME.md                     ← Dashboard/index
│
├── craft/                      ← Ardent (leather, bindery, joinery)
│   ├── leatherworks/           ← from projects/craft/handcraft/leatherworks
│   ├── bindery/                ← from projects/craft/handcraft/bindery
│   ├── joinery/                ← from projects/craft/handcraft/joinery
│   ├── cad/                    ← from projects/craft/cad
│   ├── brand/                  ← from projects/craft/handcraft/leatherworks/brand
│   ├── planning/               ← from projects/craft/handcraft/planning
│   ├── site-content/           ← from nous/demiurge/ardent_site_content
│   └── business/               ← from nous/demiurge/documents/ardent-llc
│
├── work/                       ← Summus
│   ├── sql/                    ← from projects/work (the actual SQL work)
│   ├── tracking/               ← from projects/work/chiron-tracking
│   └── outputs/                ← from projects/work/_outputs
│
├── school/                     ← MBA (TEMBA)
│   ├── sp26/                   ← current semester
│   ├── fa25/                   ← previous semester
│   ├── shared/                 ← frameworks, syllabi, references
│   └── summaries/
│
├── writing/                    ← Creative work
│   ├── echos/                  ← The Coherence of Aporia
│   ├── poetry/                 ← poetry.md + individual poems
│   └── philosophy/             ← from nous/demiurge/writing/philosophy
│
├── vehicle/                    ← 12v Cummins
│   ├── build-plan/             ← AKRON-PHASES-REVISED.md, shopping lists
│   ├── guides/                 ← install-docs, research summaries
│   ├── database/               ← DB docs (not the DB itself)
│   └── documentation/          ← manuals, specs
│
├── preparedness/               ← from projects/preparedness
│   ├── civil-rights/
│   ├── firearms/
│   └── documentation/
│
├── radio/                      ← from projects/radio
│   ├── documentation/
│   └── manuals/
│
├── career/                     ← from projects/career
│   ├── job-search/
│   ├── consulting/
│   └── interviews/
│
├── family/                     ← Syl's human-facing outputs
│   ├── cooper/                 ← schedule, development tracking
│   ├── household/              ← operations, basics
│   ├── pets/                   ← luna meds, etc.
│   └── recipes/
│
├── reference/                  ← from projects/reference
│   ├── identity/               ← cognitive profile, assessments
│   └── library/
│
├── immigration/                ← from projects/immigrate
│
├── portfolio/                  ← from projects/portfolio
│
├── metaxynoesis/               ← from projects/metaxynoesis
│
├── personal/                   ← inventory, etc.
│   └── inventory/
│
├── documents/                  ← Legal/financial docs
│   ├── ardent-llc/             ← formation, tax, agreements
│   ├── health/                 ← neuropsych eval
│   └── financial/              ← relay exports
│
└── inbox/                      ← Quick capture (from projects/inbox)
```

---

## What Stays in Nous (Agent-Only)

Per agent, these are machine-internal:

| File | Purpose | Human needs? |
|------|---------|-------------|
| SOUL.md | Agent character | No (but accessible via aletheia/ root) |
| AGENTS.md | Operating procedures | No |
| HEARTBEAT.md | Heartbeat instructions | No |
| TOOLS.md | Tool references | No |
| IDENTITY.md | Agent metadata | No |
| USER.md | Human context | No |
| MEMORY.md | Curated memory | No |
| memory/*.md | Daily session notes | No |
| memory/facts.jsonl | Structured facts | No |
| bin/ | Agent scripts | No |
| config/ | Agent configs | No |
| .task/ | Taskwarrior data | No |
| BACKLOG.md | Agent backlog | No |
| QUICK-REFERENCE.md | Agent quick ref | No |

These stay in `nous/{agent}/` and are never surfaced in the vault.

---

## Migration Plan

### Phase 1: Create vault/ with symlinks

Symlinks from vault/ → existing locations. Zero data movement.
Each vault path points to where the content already lives.

### Phase 2: Consolidate scattered human content

Move human-facing content from nous/ to vault/:
- `nous/demiurge/documents/` → `vault/documents/`
- `nous/demiurge/writing/` → `vault/writing/`
- `nous/demiurge/ardent_site_content/` → `vault/craft/site-content/`
- `nous/syl/memory/home-recipe-book.md` → `vault/family/recipes/`
- `nous/syl/memory/cooper-detailed-schedule.md` → `vault/family/cooper/`
- `nous/akron/workspace/` → `vault/vehicle/`
- Akron research → `vault/vehicle/guides/`

After moving, create symlinks FROM nous/ TO vault/ so agents still find their files.

### Phase 3: Agent format optimization

Convert agent-internal files to more efficient formats where it helps:
- Consider YAML for structured configs (TOOLS.md → tools.yaml)
- Consider structured sections for AGENTS.md
- Keep SOUL.md as prose (most effective for LLM character internalization)
- Keep memory as markdown (best for search + human debugging)

---

## Agent Impact

### Demiurge
- Loses: documents/, writing/, ardent_site_content/ (moved to vault)
- Gains: symlinks to same content in vault/
- Reads: vault/craft/*, vault/documents/*

### Syl
- Loses: human-facing memory files (recipes, schedules)
- Gains: symlinks to vault/family/*
- Reads: vault/family/*

### Akron
- Loses: workspace/ (moved to vault/vehicle/)
- Gains: symlink from workspace/ → vault/vehicle/
- Reads: vault/vehicle/*

### Chiron
- No change (projects/work/ becomes vault/work/)
- Reads: vault/work/*

### Eiron
- No change (projects/mba/ becomes vault/school/)
- Reads: vault/school/*

### Arbor
- Site code stays in nous/arbor/a2z-tree-site/ (code, not docs)
- Research could surface to vault/ if desired

### Syn
- No content in vault (orchestrator has no human-facing output)
- Reads all of vault/ for context

---

## Obsidian Config

Daily notes → `vault/inbox/` (quick capture, not agent memory)
Templates → `vault/.templates/`
Attachments → `vault/.attachments/`

No file exclusion filters needed — everything in vault/ IS human-readable by design.
