---
scope: "crates/nous/src/bootstrap/"
defers_to: ["../../../../ARCHITECTURE.md", "../../../../docs/lexicon.md"]
tightens: []
---

# Bootstrap Assembly - Two-Axis Model

The bootstrap assembler builds the system prompt from workspace files, domain packs, and `_llm/` indexes. It uses a **two-axis classification** to decide what gets included and in what order.

## Axes

| Axis | Type | What it expresses |
|------|------|-------------------|
| **Slot** | `BootstrapSlot` | **Role** - what function the section serves in the prompt |
| **Priority** | `SectionPriority` | **Importance** - how hard we fight to keep it under budget pressure |
| **Load Tier** | `LoadTier` | **Timing** - whether it loads unconditionally or only for relevant task hints |

Slot and priority are orthogonal. A file can be `Required` (priority) **and** `Context` (slot). Load tier is orthogonal to both - it gates whether the file is loaded at all, not where it sorts.

## Slot precedence

When assembled, sections sort by slot first, then by priority within the same slot. Stable sort preserves declaration order for ties.

1. `Identity` - name, emoji, avatar metadata (`IDENTITY.md`)
2. `SoulPersona` - workspace-local persona, operator-curated, per-instance (`SOUL.md`)
3. `OperatorProfile` - what the operator brings, attested (`USER.md`)
4. `Prosoche` - heartbeat / attention checklist (`PROSOCHE.md`)
5. `Team` - who else is in the workspace (`AGENTS.md`)
6. `Goals` - active / completed / deferred goals (`GOALS.md`)
7. `Tools` - registered tool surface (`TOOLS.md`)
8. `Checklist` - work procedures / checklist (`CHECKLIST.md`)
9. `Memory` - operational memory, accumulated over time (`MEMORY.md`)
10. `Context` - runtime config / auto-generated context (`CONTEXT.md`, `_llm/`, packs, `output-style`)

### Identity-related slot semantics

The first three slots are all about "who," but they have distinct semantics:

- **IDENTITY.md (`Identity`)** - *What the agent is.* Fixed metadata: name, emoji, avatar. Machine-readable, rarely changes.
- **SOUL.md (`SoulPersona`)** - *How the agent acts in this workspace.* Operator-curated, per-instance personality. This is the workspace-local persona slot. It overrides generic behavior but stays below the universal contract.
- **USER.md (`OperatorProfile`)** - *Who the operator is.* Attested profile, communication preferences, domains of expertise. The agent adapts to the operator, not the other way around.

External design prior: HKUDS/DeepTutor `BOOTSTRAP_FILES` order (`AGENTS.md` - `SOUL.md` - `USER.md` - `TOOLS.md`). Our precedence refines this with more granular role slots.

## Priority levels

- `Required` - must be included; missing = error.
- `Important` - should be included if present; missing = skip silently.
- `Flexible` - can be truncated (oldest content removed first).
- `Optional` - dropped first under budget pressure.

## Pre-injection scan

Slot orthogonality means the pre-injection scan (aletheia#184) applies **regardless of slot**. A scan finding in `SOUL.md` is handled the same way as a finding in `TOOLS.md` or `CONTEXT.md`. The scan operates on the resolved content, not on the slot classification.
