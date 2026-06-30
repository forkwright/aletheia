# Documentation map

Canonical document for each fact class. When two docs cover the same topic, the entry here
names which one is authoritative.

---

## Setup and onboarding

| Fact class | Canonical doc |
|------------|---------------|
| First-run instructions | [QUICKSTART.md](QUICKSTART.md) |
| Standard deployment | [DEPLOYMENT.md](DEPLOYMENT.md) |
| Air-gapped install | [AIR-GAPPED.md](AIR-GAPPED.md) |
| Golden path (v1.0 target) | [GOLDEN-PATH.md](GOLDEN-PATH.md) |
| Desktop preview | [DESKTOP.md](DESKTOP.md) |

## Configuration and operations

| Fact class | Canonical doc |
|------------|---------------|
| All config keys and env vars | [CONFIGURATION.md](CONFIGURATION.md) |
| Feature flags and build recipes | [FEATURE-FLAGS.md](FEATURE-FLAGS.md) |
| Network calls and air-gap paths | [NETWORK.md](NETWORK.md) |
| Runbook (day-two ops) | [RUNBOOK.md](RUNBOOK.md) |
| Hot-reload | [HOT-RELOAD.md](HOT-RELOAD.md) |
| Disaster recovery | [DISASTER-RECOVERY.md](DISASTER-RECOVERY.md) |
| Data inventory | [DATA.md](DATA.md) |
| Ingest pipeline | [INGEST.md](INGEST.md) |
| Domain packs | [PACKS.md](PACKS.md) |
| Workspace files | [WORKSPACE_FILES.md](WORKSPACE_FILES.md) |
| Upgrade guide | [UPGRADING.md](UPGRADING.md) |

## Architecture and design

| Fact class | Canonical doc |
|------------|---------------|
| Module map, crate tree, dependency graph | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Architecture walkthrough for contributors | [ARCHITECTURE-GUIDE.md](ARCHITECTURE-GUIDE.md) |
| Quick-reference crate table | [ARCHITECTURE-QUICK.md](ARCHITECTURE-QUICK.md) |
| Technology choices, dependency policy | [TECHNOLOGY.md](TECHNOLOGY.md) |
| Project framing and interface status | [PROJECT.md](PROJECT.md) |
| Storage engine evaluation | [FJALL-EVALUATION.md](FJALL-EVALUATION.md) |
| API versioning policy | [API-VERSIONING.md](API-VERSIONING.md) |
| Hubs subsystem | [HUBS.md](HUBS.md) |
| Daemon subsystem | [DAEMON.md](DAEMON.md) |
| MCP server integration | [MCP-SERVERS.md](MCP-SERVERS.md) |
| Prosoche self-audit | [PROSOCHE.md](PROSOCHE.md) |

## Observability and testing

| Fact class | Canonical doc |
|------------|---------------|
| Metrics, traces, health endpoints | [OBSERVABILITY.md](OBSERVABILITY.md) |
| Test tiers and feature gates | [test-tiers.md](test-tiers.md) |
| Benchmarks | [BENCHMARKS.md](BENCHMARKS.md) |

## Terminology and naming

| Fact class | Canonical doc |
|------------|---------------|
| Crate registry with etymology | [lexicon.md](lexicon.md) |
| Expanded term glossary | [glossary.md](glossary.md) |

NOTE: `lexicon.md` is the living registry of all crate and module names with
etymology and naming rationale. `glossary.md` provides deeper conceptual definitions
for the same terms and cross-references.

## Contributing and process

| Fact class | Canonical doc |
|------------|---------------|
| Release process | [RELEASING.md](RELEASING.md) |
| Automation PR gate and auto-merge policy | [AUTOMATION-PR-GATES.md](AUTOMATION-PR-GATES.md) |
| No-AI-attribution policy | [NO-AI-ATTRIBUTION.md](NO-AI-ATTRIBUTION.md) |
| Lessons learned | [LESSONS-LEARNED.md](LESSONS-LEARNED.md) |

## Point-in-time audit reports

These documents are dated analyses. Their findings may be resolved, partially resolved, or
tracked in GitHub issues. They are not updated to reflect current state.

| Document | Date | Status |
|----------|------|--------|
| [OBSERVABILITY-AUDIT.md](OBSERVABILITY-AUDIT.md) | 2026-04-16 | Historical; closes #3259. Open gaps tracked in GitHub issues. |
| [GRACEFUL-DEGRADATION.md](GRACEFUL-DEGRADATION.md) | Point-in-time | Historical audit; open gaps tracked in GitHub issues. |
| [GROUNDS.md](GROUNDS.md) | Point-in-time | Multiple-grounds audit; closes #3507. |
| [TRANSLATION-TAX.md](TRANSLATION-TAX.md) | Point-in-time | Boundary analysis; research for #3504. |

## Agent navigation

For AI agent navigation, start with the [`_llm/`](../_llm/) index and
[`_llm/README.md`](../_llm/README.md). The docs above are the authoritative human-readable surfaces;
`_llm/` provides derived compact indexes and the agent surface manifest.
