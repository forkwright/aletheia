# ADR-001: ADR practice for aletheia

## Status

Accepted

## Context

aletheia accretes architectural decisions across orchestration (energeia), session graph (graphe), memory facade (mneme), HTTP middleware (pylon), provider dispatch (hermeneus), and routing (aletheia-routing). Without a structured record, each new agent and contributor relitigates past choices. The cost is duplicate discussion, inconsistent implementation, and decisions that exist only in the memory of agents that have since rolled off context.

D-020 identified the need for a formal Architecture Decision Record (ADR) practice across the fleet. The kanon repo adopted the practice in its own ADR-001 and chose aletheia as the first canary for fan-out (D-020 Phase 5). If the convention works here, harmonia/akroasis/theatron follow in a later sweep. The fleet-wide `decision_index` MCP tool is deferred until at least two repos have ADRs.

Michael Nygard introduced the lightweight ADR format in 2011. It has since become the de facto standard for documenting significant architectural choices without the ceremony of full design specifications. aletheia already uses Nygard-style framing informally in concept docs and discussion-queue entries. This ADR makes the practice explicit, uniform, and machine-auditable for this repo.

## Decision

**aletheia maintains ADRs under `decisions/ADR-NNN-<slug>.md` at the repo root.**

The path differs from kanon's `projects/<repo>/decisions/` convention because aletheia is a single-repo project, not a multi-project tree like kanon. Each fleet repo locates its own `decisions/` at the natural root for its layout. The format, lifecycle, and cross-referencing rules are identical to kanon's ADR-001.

### Format

Follow the Nygard style with one fleet extension:

1. **Status** - Proposed, Accepted, Deprecated, or Superseded
2. **Context** - What forces the decision? What constraints apply?
3. **Decision** - The choice, stated boldly and precisely
4. **Consequences** - Positive and negative outcomes, including tradeoffs
5. **References** - D-NNN entries, B-NNN backlog items, PRs, concept docs, external literature

The References section is required so that agents can traverse the graph of related work. Every ADR must cite at least one source or related artifact.

### Status lifecycle

| Status | Meaning |
|--------|---------|
| Proposed | Under discussion; not yet authoritative |
| Accepted | Authoritative; guides current work |
| Deprecated | Still valid but scheduled for replacement |
| Superseded | Replaced by a newer ADR; the newer ADR is authoritative |

An ADR that moves to Superseded must list the superseding ADR in its Status section.

### Numbering and slug convention

- Numbers are sequential within aletheia, starting at 001
- Slugs are lowercase, kebab-case, and describe the decision topic
- Examples: `ADR-001-adr-practice.md`, `ADR-002-energeia-orchestrator-shape.md`

### Scope

An ADR records a decision that is:

- Architectural or structural (affects more than one module or crate)
- Long-lived (expected to persist for months or years)
- Costly to reverse (would require significant refactoring to undo)

Minor choices, experimental spikes, and local implementation details do not need ADRs. When in doubt, prefer recording the decision; the cost of an ADR is lower than the cost of rediscovering the rationale.

### Cross-referencing

ADRs are nodes in a graph, not isolated documents. Every ADR must reference:

- The D-NNN discussion thread that prompted it, if any
- Related B-NNN backlog items that track implementation
- PRs that landed the decision or changed its consequences
- Concept docs that explain the underlying pattern
- Prior ADRs that this one supersedes or extends

## Consequences

**Positive:**

- **Single source of truth for decisions.** An agent reading hermeneus or pylon for the first time can discover why the dispatch model spawns OAuth'd CLIs, why graphe and mneme are separate crates, or why pylon middleware orders the way it does, without asking.
- **Prevents relitigation.** A decision that is recorded and accepted does not need to be re-discussed unless new evidence emerges.
- **Enables safe deprecation.** When a decision is superseded, the old ADR remains in the archive with a pointer to the replacement. History is preserved.
- **Canary for fleet-wide adoption.** If the pattern works in aletheia, harmonia/akroasis/theatron get a proven template instead of re-deriving one.

**Negative:**

- **Maintenance burden.** ADRs drift from code if not updated when implementations change. Structural completeness can be lint-enforced; semantic freshness requires human or agent review.
- **Scope creep.** Contributors may record too many minor decisions, diluting the signal. The scope criteria above are the guardrail.
- **Delay.** A "Proposed" ADR can become a bottleneck if authors wait for consensus before writing code. The rule is: write the ADR when the decision crystallizes, not before.
- **Empty scaffold risk.** This canary lands the practice without ADR-002/003/... yet. The directory must not become an abandoned shell; #4039 tracks the follow-up backfill of the first three substantive ADRs (orchestrator shape, graphe/mneme split, pylon middleware ordering).

## References

- D-020 (DDD, topology, gnomon philosophy, ADR practice; Phase 5 = aletheia canary)
- forkwright/aletheia#4039 (this canary; tracks backfill of ADR-002/003/004)
- forkwright/kanon `projects/kanon/decisions/ADR-001-adr-practice.md` (fleet template)
- [Michael Nygard, "Documenting Architecture Decisions" (2011)](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
