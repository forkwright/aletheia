# Specifications

Design specs for Aletheia's architecture, security model, and subsystems.

These are **internal design documents** — they describe how things should work,
what constraints exist, and what tradeoffs were made. They're living documents
that evolve with the system.

## Active Specs

| # | Spec | Status | Remaining |
|---|------|--------|-----------|
| 12 | [Session Continuity](12_session-continuity.md) | Phases 1-4, 6-9 done | Recency-boosted recall (5), distillation progress UI (10) |
| 13 | [Sub-Agent Workforce](13_sub-agent-workforce.md) | ✅ Complete | All 7 phases done |
| 4 | [Cost-Aware Orchestration](04_cost-aware-orchestration.md) | Phase 1 done, Phase 2 partial | Plan mode, automatic model routing per-turn, cost visibility/tracking |
| 3 | [Auth & Updates](03_auth-and-updates.md) | Auth done (2a-2e) | Part 2: release workflow, `aletheia update` CLI, update check daemon. Also: migrate-auth CLI, session mgmt UI |
| 7 | [Knowledge Graph](07_knowledge-graph.md) | Phase 1a-1b, 2a done | Memory confidence/decay, graph UI search+edit, domain scoping, thread-aware recall |
| 11 | [Chat Output Quality](11_chat-output-quality.md) | Phases 1-4 done | Rich message components (Phase 5) |
| 9 | [Graph Visualization](09_graph-visualization.md) | Phase 1-3 done | Named communities, semantic node cards, search, editing, auditing, archaeology, cross-agent viz, drift detection |
| 16 | [Efficiency](16_efficiency.md) | Phases 1-3 done | Cost visibility (4) |
| 15 | [UI Interaction Quality](15_ui-interaction-quality.md) | Phases 1-3 done | Tool categorization & grouping (Phase 4), status line enhancement (Phase 5) |
| 14 | [Development Workflow](14_development-workflow.md) | Phases 1-4, 6 done | Versioning + releases (Phase 5) |
| 5 | [Plug-and-Play Onboarding](05_plug-and-play-onboarding.md) | Draft | Everything — zero implementation |

### Priority order

1. **14 Development Workflow** — Process. Every other spec ships through this pipeline. Fix the pipeline first: spec template, branch/PR convention, CI zero-failures, automated versioning, agent task dispatch. Without this, every spec creates cleanup debt.
2. **12 Session Continuity** — Foundational. The session IS the agent. If distillation is broken, nothing else matters — context degrades, memory has gaps, the conversation doesn't feel continuous.
3. **13 Sub-Agent Workforce** — Efficiency multiplier. Delegation reduces context pressure on the primary session, cuts cost 40-60%, and lets me stay present in conversation while work happens in parallel.
4. **4 Cost-Aware Orchestration** — Complements #13. Plan mode gives Cody visibility into what I'm about to do. Model routing becomes simpler once sub-agents handle the cheap work.
5. **3 Auth & Updates** — Operations. The update CLI eliminates manual deploys. Not blocking development, but reduces friction for every future change.
6. **7 Knowledge Graph** — Memory quality. Making Neo4j optional reduces infrastructure burden. Better extraction improves what survives distillation.
7. **16 Efficiency** — Performance. Parallel tool execution (2-5x faster tool-heavy turns), token audit + truncation, dynamic thinking budget. Compounds on every turn.
8. **15 UI Interaction Quality** — Polish. Thinking persistence + formatting and tool input display are small changes with outsized impact on the chat experience. Groups naturally with #11.
9. **11 Chat Output Quality** — Polish. Runtime narration filter is a safety net for prompt compliance. Rich components improve the conversation experience.
10. **9 Graph Visualization** — Deep feature work. Depends on knowledge graph backend (#7). High value but lower urgency.
11. **5 Plug-and-Play Onboarding** — Capstone. Ship last, after everything else is solid enough for someone else to run.

## Implemented (Archived)

| Spec | Implemented | Summary |
|------|-------------|---------|
| [Webchat UX](archive/02_webchat-ux.md) | PR #47 | SSE notifications, refresh resilience, tool output fix, file editor (CodeMirror 6) |
| [Turn Safety](archive/01_turn-safety.md) | PR #38 + #39 | Error propagation, distillation guards, orphan diagnostics, duplicate tool_result fix |
| [Data Privacy](archive/spec-data-privacy.md) | PR #33 | File permissions hardening, retention policy, log sanitization, encrypted export, sidecar auth |
| [Unified Thread Model](archive/spec-unified-thread-model.md) | PR #32 | Transport isolation, thread abstraction, thread summaries, topic branching (all 4 phases) |
| [Code Quality](archive/06_code-quality.md) | PRs #37, #45, #52, #60, #62 | Error taxonomy, dead code audit, CONTRIBUTING.md, error handling sweep, oxlint enforcement |
| [Memory Continuity](archive/08_memory-continuity.md) | PRs #36, #43, #44, #55 | Expanded tail, structured summaries, context editing API, working state, agent notes |
| [Thinking UI](archive/10_thinking-ui.md) | PRs #40, #54, #63 | Extended thinking for Opus, status pills + detail panel, collapsed reasoning in history |
| [Auth & Security](archive/spec-auth-and-security.md) | PR #26 + security commits | JWT, RBAC, sessions, audit, TLS, passwords (standalone modules; integration pending) |
| [Modular Runtime Architecture](archive/spec-modular-runtime-architecture.md) | PR #21 | Pipeline decomposition, composable stages |
| [Tool Call Governance](archive/spec-tool-call-governance.md) | PR #22 | Approval gates, timeouts, LoopDetector |
| [Distillation & Memory Persistence](archive/spec-distillation-memory-persistence.md) | Hooks | Workspace flush on distillation |

## Conventions

- **Filename:** `NN_<topic>.md` (numbered by implementation order)
- **Status:** Draft → In Progress → Implemented → Archived
- **Format:** Problem statement → Design → Constraints → Open questions
- Specs describe *intent and design*, not implementation. Code is the source of truth for how things actually work.
- Implemented specs move to `archive/` with a note on which PR delivered them.

## Adding a Spec

1. Create `NN_<topic>.md` in this directory (next available number)
2. Add it to the index above
3. Start with Draft status
4. PR for review when ready
