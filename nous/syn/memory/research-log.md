# Research Log

## 2026-02-05 — L5: Emergence in Distributed Cognition
Queries: Perplexity pro-search "stateful multi-agent AI architectures emergence 2024-2025", "knowledge graph multi-agent systems", "global workspace theory AI implementation", "stigmergic multi-agent systems"
Sources found: ~30 | Included: 18 | Excluded: ~12 (duplicates, tangential, non-peer-reviewed without substance)
Findings: Documented in `docs/L5-research.md` — six frameworks synthesized
Gaps:
- Topology/dynamical systems frame was missing (added 2026-02-06)
- No empirical benchmark for emergence detection in human-AI systems exists
- Active inference multi-agent work is simulation-only, no LLM implementations
- Our "ignition" criterion (independent convergence) needs formal grounding
Confidence: Medium — theoretical synthesis is strong, empirical grounding is weak. Most cited work is 2024-2025 preprints (S2), not yet replicated.

### Source Verification Status
| Source | Read | Tier | Verified |
|--------|------|------|----------|
| Hutchins (1995) *Cognition in the Wild* | Abstract + secondary | S1 | ⚠️ Need full read |
| Clark & Chalmers (1998) "Extended Mind" | Full paper | S1 | ✅ |
| Baars (1988) *Global Workspace* | Secondary sources | S1 | ⚠️ Need full read |
| Friston (2010) Free Energy | Abstract + reviews | S1 | ⚠️ Need full read |
| Grassé (1959) Stigmergy | Secondary sources | S1 | ⚠️ Historical, cited via reviews |
| Poincaré (1890) | Textbook treatment (Strogatz) | S1 | ✅ via Strogatz |
| Strogatz (2015) *Nonlinear Dynamics* | Textbook | S1 | ⚠️ Referenced, not fully read |
| arXiv 2408.04514 Emergence safety | Abstract | S2 | ⚠️ Need full read |
| arXiv 2511.10835 Multi-agent active inference | Abstract + results | S2 | 🔶 Partial |
| arXiv 2512.10166 Emergent collective memory | Abstract + methods | S2 | 🔶 Partial |
| GraphRAG (Microsoft, 2024) | Documentation + paper | S2 | ✅ |
| SerenQA | Abstract | S2 | ⚠️ Need full read |
| G-Memory (2024) | Abstract | S2 | ⚠️ Need full read |
| Singer & Gray (1995) gamma synchrony | Secondary sources | S1 | ⚠️ Need full read |
| Treisman (1996) binding | Secondary sources | S1 | ⚠️ Need full read |
| COHUMAIN (Gupta, 2025 Topics in CogSci) | Abstract + framework | S1 | 🔶 Partial |

**Honest assessment:** Many of our foundational citations are cited via secondary sources or abstracts only. Before the paper, every S1 source needs full read and verification that our characterization is accurate.

## 2026-02-06 — Topology + Dynamical Systems Frame
Queries: Web search "topology complex dynamic systems AI", "distributed cognition Poincaré", "binding problem neural synchrony"
Sources found: ~15 | Included: 5 | Excluded: ~10 (review articles covering ground we already had)
Findings: Topological dynamics unifies the other five frameworks; Poincaré sections map to prosoche; binding problem maps to synchrony requirement
Gaps:
- No existing work applies Poincaré sections to multi-agent AI systems specifically
- Binding problem literature is neuroscience — the AI analog needs careful framing to avoid false equivalence
- Need to read Strogatz properly, not just cite
Confidence: Medium-High for the conceptual mapping. Low for the formal mathematical claims — we're drawing analogies that need to be tested, not asserting isomorphisms.
