// poiesis-typst eval-report template.
//
// Renders eval framework benchmark results to a structured PDF report.
// Expected JSON schema:
// {
//   "summary": { "passed": n, "failed": n, "skipped": n, "total_duration_ms": n },
//   "benchmarks": [
//     { "id": "test-id", "category": "category", "outcome": "passed|failed|skipped",
//       "duration_ms": n_or_null, "error": "msg_or_null", "skip_reason": "msg_or_null" }
//   ]
// }

#let data = json("data.json")

#set page(paper: "us-letter", margin: 0.75in)
#set text(font: "Liberation Sans", size: 10pt)
#set par(leading: 0.65em, spacing: 0.8em)

// Title block
#align(left)[
  #text(18pt, weight: "bold")[Evaluation Report]
]

#v(16pt)

// Summary section
#if "summary" in data [
  #let s = data.summary
  #text(12pt, weight: "bold")[Summary]
  #v(4pt)
  #text(10pt)[
    Passed: #s.passed | Failed: #s.failed | Skipped: #s.skipped \
    Total Duration: #(s.total_duration_ms)ms
  ]
  #v(12pt)
]

// Benchmark results table
#if "benchmarks" in data [
  #let benchmarks = data.benchmarks
  #text(12pt, weight: "bold")[Results]
  #v(4pt)

  #table(
    columns: (2fr, 1fr, 1fr),
    fill: (_, y) => if y == 0 { rgb("#333333") } else { none },
    stroke: 0.5pt + rgb("#cccccc"),
    inset: 6pt,
    text(weight: "bold", fill: white, "Test ID"),
    text(weight: "bold", fill: white, "Outcome"),
    text(weight: "bold", fill: white, "Duration"),
    ..benchmarks.map(b => {
      let color = if b.outcome == "passed" {
        rgb("#00aa00")
      } else if b.outcome == "failed" {
        rgb("#ff0000")
      } else {
        rgb("#ffaa00")
      }
      let duration_text = if b.duration_ms != none {
        str(b.duration_ms) + "ms"
      } else {
        "—"
      }
      (b.id, text(fill: color, b.outcome), duration_text)
    }).flatten(),
  )
]
