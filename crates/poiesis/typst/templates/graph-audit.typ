// poiesis-typst graph-audit template.
//
// Renders knowledge-graph audit findings to a structured PDF report.
// Expected JSON schema:
// {
//   "summary": { "total": n, "by_scope": { "crate": n, "module": n, "concept": n, "boundary": n } },
//   "facts": [
//     { "id": "fact.id", "scope": "crate|module|concept|boundary", "claim": "text",
//       "evidence": ["path/file.rs", "url"], "updated_at": "2026-04-22T...", "updated_by": "PR-123" }
//   ]
// }

#let data = json("data.json")

#set page(paper: "us-letter", margin: 0.75in)
#set text(font: "Liberation Sans", size: 10pt)
#set par(leading: 0.65em, spacing: 0.8em)

// Title block
#align(left)[
  #text(18pt, weight: "bold")[Architecture Fact Audit]
]

#v(16pt)

// Summary section
#if "summary" in data [
  #let s = data.summary
  #let by_scope = s.at("by_scope", default: (:))
  #text(12pt, weight: "bold")[Summary]
  #v(4pt)
  #text(10pt)[
    Total facts: #s.total \
    Crate: #(by_scope.at("crate", default: 0)) |
    Module: #(by_scope.at("module", default: 0)) |
    Concept: #(by_scope.at("concept", default: 0)) |
    Boundary: #(by_scope.at("boundary", default: 0))
  ]
  #v(12pt)
]

// Facts section
#if "facts" in data [
  #let facts = data.facts
  #text(12pt, weight: "bold")[Facts]
  #v(4pt)

  #for fact in facts [
    #text(weight: "bold", size: 10pt)[#fact.id]
    #v(2pt)
    #text(9pt, style: "italic", fill: rgb("#666666"))[#fact.scope | Updated: #fact.updated_at by #fact.updated_by]
    #v(2pt)
    #text(10pt)[#fact.claim]

    #if fact.evidence.len() > 0 [
      #text(9pt, fill: rgb("#666666"))[
        Evidence: #fact.evidence.join(", ")
      ]
    ]

    #v(8pt)
  ]
]
