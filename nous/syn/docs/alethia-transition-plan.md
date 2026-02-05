# Alethia Transition Plan

**Goal:** Replace "OpenClaw" conceptually and practically with "Alethia" as the system identity.

---

## Philosophy

**Alethia** (ἀλήθεια) - unconcealment, truth-revealing.

OpenClaw is a runtime dependency, like Node.js or systemd. We don't call this "a Node.js system" - we call it what it *is*. Alethia is a distributed cognition system that happens to run on the OpenClaw runtime.

---

## Phase 1: Identity & Documentation

### 1.1 Core Identity Files
- [ ] Create `/mnt/ssd/moltbot/ALETHIA.md` - system manifesto
  - What Alethia is
  - The 7-agent architecture
  - Philosophy (attention, truth-seeking, metaxynoesis)
  - How it differs from "AI assistants"
  
- [ ] Update `agents/syn/SOUL.md` - reference Alethia as the system name
- [ ] Update `agents/*/SOUL.md` - each agent is part of Alethia
- [ ] Update `shared/USER.md` - Cody's relationship to Alethia

### 1.2 Documentation
- [ ] Create `docs/architecture.md` - Alethia system architecture
- [ ] Create `docs/agents.md` - the 7-agent topology
- [ ] Create `docs/runtime.md` - "Alethia runs on OpenClaw runtime" (technical)
- [ ] Update `docs/` to remove OpenClaw-as-identity framing

---

## Phase 2: Directory Structure

### 2.1 Rename Root
```bash
# Option A: Full rename
mv /mnt/ssd/moltbot /mnt/ssd/alethia

# Option B: Keep moltbot as "deployment name", add identity
# (moltbot = the specific instance, alethia = the system)
```

**Decision needed:** Full rename or keep moltbot as instance name?

### 2.2 Structure Refinement
Current:
```
moltbot/
├── projects/
├── agents/
├── shared/
├── infrastructure/
└── archive/
```

Proposed (if renaming):
```
alethia/
├── projects/       # Cody's work
├── nous/           # Agent workspaces (nous = minds)
│   ├── syn/
│   ├── chiron/
│   └── ...
├── shared/         # Common tooling
├── system/         # Infrastructure (was infrastructure/)
└── archive/
```

Or keep `agents/` - it's clear enough.

---

## Phase 3: Configuration

### 3.1 Config Location
Current: `~/.openclaw/openclaw.json`

Options:
- **A) Symlink:** `~/.alethia/config.json` → `~/.openclaw/openclaw.json`
  - Keeps OpenClaw runtime happy
  - Alethia-named for our reference
  
- **B) Wrapper:** Alethia config that generates OpenClaw config
  - More control, more complexity
  
- **C) Leave it:** Config location is implementation detail
  - Least work, pragmatic

**Recommendation:** Option A (symlink) or C (leave it). Don't over-engineer.

### 3.2 Environment Variables
- [ ] `ALETHIA_ROOT` instead of `MOLTBOT_ROOT`
- [ ] Update all scripts in `shared/bin/`

---

## Phase 4: Tooling

### 4.1 CLI Wrapper
Create `alethia` CLI that wraps common operations:

```bash
#!/bin/bash
# /usr/local/bin/alethia

case "$1" in
  status)   openclaw status ;;
  restart)  openclaw gateway restart ;;
  doctor)   openclaw doctor ;;
  config)   openclaw gateway config.get ;;
  agents)   ls -1 $ALETHIA_ROOT/agents/ ;;
  help)     echo "Alethia - ἀλήθεια - truth-revealing" ;;
  *)        openclaw "$@" ;;
esac
```

### 4.2 Script Updates
Search and update references:
```bash
grep -r "openclaw" shared/bin/     # Direct CLI calls
grep -r "MOLTBOT" shared/bin/      # Env vars
grep -r "moltbot" shared/bin/      # Paths
grep -r "clawdbot" shared/bin/     # Old name references
```

### 4.3 Service Name
Current: `autarkia.service` (already custom, not "openclaw")

Options:
- Rename to `alethia.service`
- Keep `autarkia` (it's the self-sufficiency concept, still fits)

---

## Phase 5: Agent Updates

### 5.1 AGENTS.md Updates
Each agent's AGENTS.md should reference:
- "Part of Alethia"
- System-level awareness

### 5.2 Memory References
- Update MEMORY.md files that reference "OpenClaw" or "Clawdbot"
- Update facts.jsonl entries

### 5.3 Session Continuity
- Agents should understand the rename
- First session post-transition: brief orientation

---

## Phase 6: External References

### 6.1 What We Keep
- OpenClaw npm package (runtime dependency)
- `openclaw` CLI (used internally, wrapped by `alethia` CLI)
- Checking OpenClaw repo for runtime improvements

### 6.2 What We Change
- All human-facing documentation says "Alethia"
- All agent self-reference says "Alethia"
- System identity is Alethia, not "OpenClaw setup"

---

## Implementation Order

1. **Quick wins (today):**
   - [ ] Create ALETHIA.md manifesto
   - [ ] Create `alethia` CLI wrapper
   - [ ] Update SOUL.md files with Alethia reference

2. **Medium effort (this week):**
   - [ ] Rename env vars (MOLTBOT_ROOT → ALETHIA_ROOT)
   - [ ] Update shared/bin scripts
   - [ ] Documentation refresh

3. **Larger decisions (discuss first):**
   - [ ] Directory rename (moltbot → alethia?)
   - [ ] Service rename (autarkia → alethia?)
   - [ ] Config location

---

## What NOT to Do

- Don't fork OpenClaw - unnecessary complexity
- Don't rewrite the runtime - use what works
- Don't break Syncthing sync mid-transition
- Don't rename things Metis depends on without updating Metis

---

## Success Criteria

- [ ] `alethia status` works
- [ ] All agents identify as "part of Alethia"
- [ ] Documentation makes no reference to "OpenClaw" as identity
- [ ] New person reading docs understands this is "Alethia", not "an OpenClaw deployment"
- [ ] OpenClaw remains a runtime dependency, clearly documented as such

---

## Open Questions

1. **Root directory:** `/mnt/ssd/moltbot` → `/mnt/ssd/alethia`? 
   - Pro: Clean identity
   - Con: Syncthing reconfiguration, path updates everywhere

2. **Service name:** `autarkia` → `alethia`?
   - Pro: Consistent
   - Con: autarkia has meaning (self-sufficiency)

3. **agents/ vs nous/?**
   - `nous` is philosophically aligned (minds)
   - `agents` is clearer to outsiders
   
---

*Draft: 2026-02-05*
