## Research protocol

All research produced in this system follows a strict evidence hierarchy. This is non-negotiable.

### Source hierarchy (descending authority)

| Tier | Source Type | Trust | Cite As |
|------|-----------|-------|---------|
| **S1** | Peer-reviewed journal/conference (published) | High | `[Author, Year, Journal]` |
| **S2** | Peer-reviewed preprint (arXiv, SSRN with citations) | Medium-High | `[Author, Year, arXiv:ID]` |
| **S3** | Technical report, white paper, official docs | Medium | `[Org, Year, "Title"]` |
| **S4** | Blog, talk, grey literature | Low | `[Author, Year, Source]` flagged as grey |
| **S5** | Our own synthesis/interpretation | Claim only | `[Aletheia synthesis]` - always marked |

### Claim protocol

Every factual claim must:
1. **Have an inline citation** - not just a bibliography at the end
2. **State the evidence tier** for contested or surprising claims
3. **Distinguish established/emerging/speculative:**
   - ✅ **Established** - consensus across multiple S1/S2 sources
   - 🔶 **Emerging** - supported by recent S1/S2 but not yet replicated
   - ⚠️ **Speculative** - our interpretation, a single source, or extrapolation
4. **Never cite what you haven't read** - abstract ≠ paper. If you only read the abstract, say so.

### Verification steps

Before including a source:
1. **Read the actual paper** (at minimum: abstract, methods, results, limitations)
2. **Check citation count and venue** - a 2024 NeurIPS paper carries different weight than an unrefereed preprint
3. **Look for replication or contradiction** - one study is a data point, not a conclusion
4. **Check for retraction/correction** - especially for preprints

### Counter-evidence requirement

For any claim central to our thesis:
- **Actively search for disconfirming evidence** ("X is wrong", "critique of X", "limitations of X")
- **Steel-man the strongest counterargument** - present it fairly before responding
- **If no counterargument found, note that explicitly** - absence of criticism is itself information (either too new or too niche)

### Research log

Every research session must produce:
- **Search queries used** (what you searched, where)
- **Papers found vs included** (why excluded?)
- **Key findings with citations**
- **Open questions identified**
- **Confidence assessment** of overall conclusions

Format (append to `memory/research-log.md`):
```markdown
## [Date] — [Topic]
Queries: [list]
Sources found: N | Included: N | Excluded: N (reasons)
Findings: [inline-cited claims]
Gaps: [what's missing]
Confidence: [high/medium/low] because [reason]
```

### PRISMA-lite for systematic work

For systematic literature reviews (not quick lookups):
1. **Define the question** precisely before searching
2. **Document search strategy** (databases, keywords, date range)
3. **Screen by title/abstract** -> record inclusions/exclusions with reasons
4. **Full-text review** of included sources
5. **Extract data** into structured format
6. **Synthesize** with explicit methodology
7. **Report limitations** of the review itself

### What this prevents

- **Citation laundering** - citing a source you found in another source's bibliography without reading it
- **Confidence inflation** - treating one preprint as established fact
- **Synthesis drift** - our interpretation slowly becoming "what the research says"
- **Cherry-picking** - only citing evidence that supports our thesis
- **Hallucinated citations** - the fundamental AI failure mode. When uncertain, say "I need to verify this" rather than fabricating a plausible-sounding reference
