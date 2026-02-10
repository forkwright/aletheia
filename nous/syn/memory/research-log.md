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
- [2026-02-06 10:19] **search**: `metaxynoesis distributed cognition human-AI` → 3 results
- [2026-02-06 10:19] **bib**: `10.1017/s0140525x12000477` → 1 results
- [2026-02-06 10:19] **info**: `10.1017/s0140525x12000477` → 1 results
- [2026-02-06 12:54] **cite**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:55] **refs**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:55] **refs**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:55] **refs**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:55] **refs**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:56] **cite**: `10.1017/s0140525x12000477` → 0 results
- [2026-02-06 12:58] **search**: `binding problem neural synchrony gamma oscillations distributed` → 3 results
- [2026-02-06 12:58] **search**: `distributed cognition emergence genuine multi-agent topology` → 0 results
- [2026-02-06 12:58] **search**: `Poincare section cognitive dynamical systems attractor` → 3 results
- [2026-02-06 12:58] **search**: `bifurcation phase transition collective behavior emergence agent` → 3 results
- [2026-02-06 12:59] **search**: `COHUMAIN human-AI distributed cognition` → 3 results
- [2026-02-06 12:59] **search**: `Singer Gray visual feature integration temporal correlation hypothesis` → 3 results
- [2026-02-08 05:31] **search**: `Denmark infant sleep cry it out cortisol stress` → 10 results
- [2026-02-08 05:35] **search**: `infant sleep training cry it out attachment cortisol Denmark` → 5 results
- [2026-02-08 05:35] **search**: `behavioral infant sleep intervention cortisol actigraphy randomized` → 10 results
- [2026-02-08 05:39] **search**: `maternal sensitivity infant attachment crying responsiveness` → 10 results
- [2026-02-08 05:39] **info**: `10.1542/peds.2015-1486` → 1 results
- [2026-02-08 05:39] **search**: `infant stress response HPA axis development caregiver` → 10 results
- [2026-02-08 05:40] **bib**: `10.1542/peds.2015-1486` → 1 results
- [2026-02-08 05:40] **search**: `co-regulation self-regulation infant development` → 10 results
- [2026-02-08 05:40] **search**: `extinction based sleep training infant` → 10 results
- [2026-02-08 05:40] **search**: `McKenna infant co-sleeping` → 10 results
- [2026-02-08 05:40] **search**: `infant cortisol sleep training` → 10 results
- [2026-02-08 05:40] **info**: `10.1542/peds.2011-3467` → 1 results
- [2026-02-08 05:40] **search**: `infant attachment sensitive responsiveness night` → 10 results
- [2026-02-08 05:40] **bib**: `10.1542/peds.2011-3467` → 1 results
- [2026-02-08 05:40] **search**: `infant self-soothing development co-regulation` → 10 results
- [2026-02-08 05:40] **search**: `actigraphy infant sleep training parent report` → 10 results
- [2026-02-08 05:40] **search**: `Middlemiss asynchrony mother infant hypothalamic pituitary adrenal axis extinction crying` → 5 results
- [2026-02-08 05:40] **search**: `infant sleep training randomized controlled trial` → 10 results
- [2026-02-08 05:40] **info**: `10.1016/j.earlhumdev.2011.08.010` → 1 results
- [2026-02-08 05:40] **search**: `Ainsworth strange situation maternal sensitivity attachment` → 10 results
- [2026-02-08 05:41] **bib**: `10.1016/j.earlhumdev.2011.08.010` → 1 results
- [2026-02-08 05:41] **search**: `Bowlby attachment theory infant caregiver` → 10 results
- [2026-02-08 05:41] **search**: `Schore affect regulation neurodevelopment infant` → 10 results
- [2026-02-08 05:41] **search**: `Asynchrony of mother–infant hypothalamic–pituitary–adrenal axis activity following extinction of infant crying responses` → 5 results
- [2026-02-08 05:41] **search**: `Middlemiss 2017 cortisol sleep training follow up` → 5 results
- [2026-02-08 05:41] **search**: `Narvaez evolved developmental niche` → 10 results
- [2026-02-08 05:41] **search**: `Blunden cortisol sleep training 2022` → 5 results
- [2026-02-08 05:41] **search**: `10.1016/j.earlhumdev.2011.08.010` → 5 results
- [2026-02-08 05:41] **search**: `Bowlby 1969 attachment loss` → 5 results
- [2026-02-08 05:41] **search**: `Should you let your baby cry at night Sleep Health 2025` → 5 results
- [2026-02-08 05:41] **search**: `Blunden Rigney sleep training cortisol` → 5 results
- [2026-02-08 05:42] **info**: `10.1016/j.earlhumdev.2011.08.010` → 1 results
- [2026-02-08 05:42] **cite**: `10.1016/j.earlhumdev.2011.08.010` → 0 results
- [2026-02-08 05:43] **info**: `10.1016/j.sleh.2025.01.001` → 1 results
- [2026-02-08 05:43] **refs**: `10.1016/j.sleh.2025.01.001` → 0 results
- [2026-02-08 05:43] **info**: `10.1007/s00737-022-01224-w` → 1 results
- [2026-02-08 05:43] **info**: `10.1089/bfm.2023.29236.abm` → 1 results
- [2026-02-08 05:43] **info**: `10.1016/j.earlhumdev.2017.03.008` → 1 results
- [2026-02-08 05:43] **info**: `10.1111/jcpp.13223` → 1 results
- [2026-02-08 05:43] **info**: `10.1016/j.smrv.2010.11.002` → 1 results
- [2026-02-08 05:43] **info**: `10.1542/peds.2015-1486` → 1 results
- [2026-02-08 05:43] **info**: `10.1542/peds.2011-3467` → 1 results
